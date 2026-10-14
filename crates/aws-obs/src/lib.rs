//! AWS observability access: CloudWatch Logs + X-Ray.
//!
//! Responsibilities:
//! - Load AWS config via the standard credential chain (profiles, SSO).
//! - Tail/search log groups (`FilterLogEvents`, Logs Insights `StartQuery`).
//! - Fetch trace summaries and full traces (`GetTraceSummaries`, `BatchGetTraces`).
//!
//! Pure helpers (time parsing, response shaping) are unit-tested with no
//! credentials. Live calls require AWS config and are exercised manually.
//!
//! See `docs/PRD.md` for the product context.

use serde::Serialize;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ObsError {
    #[error("aws error: {0}")]
    Aws(String),
    #[error("invalid time argument {input:?}: {reason}")]
    InvalidTime { input: String, reason: String },
    #[error("trace not found: {0}")]
    TraceNotFound(String),
    #[error("insights query failed: {0}")]
    InsightsFailed(String),
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

/// Profile / region overrides; otherwise the standard chain is used.
#[derive(Debug, Clone, Default)]
pub struct AwsOptions {
    pub profile: Option<String>,
    pub region: Option<String>,
}

impl AwsOptions {
    pub fn new(profile: Option<String>, region: Option<String>) -> Self {
        Self { profile, region }
    }
}

/// Load SDK config via the standard chain with optional overrides.
pub async fn load_sdk_config(opts: &AwsOptions) -> aws_config::SdkConfig {
    let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
    if let Some(profile) = &opts.profile {
        loader = loader.profile_name(profile);
    }
    if let Some(region) = &opts.region {
        loader = loader.region(aws_config::Region::new(region.clone()));
    }
    loader.load().await
}

pub fn cloudwatch_client(sdk: &aws_config::SdkConfig) -> aws_sdk_cloudwatchlogs::Client {
    aws_sdk_cloudwatchlogs::Client::new(sdk)
}

pub fn xray_client(sdk: &aws_config::SdkConfig) -> aws_sdk_xray::Client {
    aws_sdk_xray::Client::new(sdk)
}

// ---------------------------------------------------------------------------
// Time helpers (pure, no AWS)
// ---------------------------------------------------------------------------

/// Current time as epoch milliseconds.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Parse a human duration like `30s`, `15m`, `3h`, `2d`, `1w`.
pub fn parse_back_duration(s: &str) -> Result<Duration, ObsError> {
    let input = s.trim().to_lowercase();
    if input.is_empty() {
        return Err(ObsError::InvalidTime {
            input: s.to_string(),
            reason: "empty duration (try 30m, 3h, 2d)".to_string(),
        });
    }
    let split = input
        .find(|c: char| c.is_alphabetic())
        .unwrap_or(input.len());
    let (num_part, unit_part) = input.split_at(split);
    let amount: u64 = num_part.parse().map_err(|_| ObsError::InvalidTime {
        input: s.to_string(),
        reason: format!("expected a number prefix in {s:?}"),
    })?;
    let secs_per_unit: u64 = match unit_part.trim() {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => 1,
        "m" | "min" | "mins" | "minute" | "minutes" => 60,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3600,
        "d" | "day" | "days" => 86_400,
        "w" | "week" | "weeks" => 604_800,
        other => {
            return Err(ObsError::InvalidTime {
                input: s.to_string(),
                reason: format!("unknown unit {other:?} (use s/m/h/d/w)"),
            });
        }
    };
    let secs = amount
        .checked_mul(secs_per_unit)
        .ok_or_else(|| ObsError::InvalidTime {
            input: s.to_string(),
            reason: "duration overflow".to_string(),
        })?;
    Ok(Duration::from_secs(secs))
}

/// Parse an absolute time: epoch secs/ms or RFC 3339. Returns epoch millis.
pub fn parse_time_arg(s: &str) -> Result<i64, ObsError> {
    let input = s.trim();
    let invalid = |reason: String| ObsError::InvalidTime {
        input: s.to_string(),
        reason,
    };
    // Bare integers: 10-digit seconds vs 13-digit millis (allow any size).
    if let Ok(num) = input.parse::<i64>() {
        if num.abs() >= 1_000_000_000_000 {
            return Ok(num);
        }
        if num.abs() >= 1_000_000_000 {
            return Ok(num.saturating_mul(1000));
        }
        return Err(invalid(
            "epoch value too small (expected seconds or milliseconds)".to_string(),
        ));
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(input) {
        return Ok(dt.timestamp_millis());
    }
    // Friendly fallback: `2006-01-02 15:04:05` assumed UTC.
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M:%S") {
        return Ok(naive.and_utc().timestamp_millis());
    }
    if let Ok(date) = chrono::NaiveDate::parse_from_str(input, "%Y-%m-%d") {
        return Ok(date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis());
    }
    Err(invalid(
        "expected epoch secs/ms or RFC 3339 (e.g. 2026-10-01T00:00:00Z)".to_string(),
    ))
}

/// Resolve `(start_ms, end_ms)` from `--back` / `--start` / `--end`.
///
/// - `--end` defaults to `now_ms`.
/// - `--start` wins over `--back`; `--back` defaults to `1h`.
pub fn resolve_time_window(
    back: Option<&str>,
    start: Option<&str>,
    end: Option<&str>,
    now_ms: i64,
) -> Result<(Option<i64>, Option<i64>), ObsError> {
    let end_ms = match end {
        Some(e) => Some(parse_time_arg(e)?),
        None => Some(now_ms),
    };
    if let Some(s) = start {
        let start_ms = parse_time_arg(s)?;
        if let Some(e) = end_ms
            && start_ms > e
        {
            return Err(ObsError::InvalidTime {
                input: s.to_string(),
                reason: "start is after end".to_string(),
            });
        }
        return Ok((Some(start_ms), end_ms));
    }
    let back_str = back.unwrap_or("1h");
    let duration = parse_back_duration(back_str)?;
    let start_ms = end_ms.map(|e| e.saturating_sub(duration.as_millis() as i64));
    Ok((start_ms, end_ms))
}

// ---------------------------------------------------------------------------
// CloudWatch Logs
// ---------------------------------------------------------------------------

/// A single log event in CLI-friendly form.
#[derive(Debug, Clone, Serialize)]
pub struct LogEvent {
    pub timestamp_ms: i64,
    pub message: String,
    pub log_group: String,
    pub log_stream: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
}

/// Query for `FilterLogEvents` (default path, FR-13).
#[derive(Debug, Clone, Default)]
pub struct LogsQuery {
    pub log_groups: Vec<String>,
    pub start_ms: Option<i64>,
    pub end_ms: Option<i64>,
    pub filter_pattern: Option<String>,
    pub limit: Option<i32>,
    pub next_token: Option<String>,
}

/// One page of `FilterLogEvents` results.
#[derive(Debug, Clone, Default)]
pub struct LogsPage {
    pub events: Vec<LogEvent>,
    pub next_token: Option<String>,
}

fn map_aws_err<E: std::fmt::Display>(e: E) -> ObsError {
    ObsError::Aws(e.to_string())
}

/// Run `FilterLogEvents` once (single page). Multi-group queries fan out per
/// group and merge by timestamp so pagination stays simple.
pub async fn filter_log_events(
    client: &aws_sdk_cloudwatchlogs::Client,
    query: &LogsQuery,
) -> Result<LogsPage, ObsError> {
    if query.log_groups.is_empty() {
        return Ok(LogsPage::default());
    }
    // One request per group keeps `next_token` semantics manageable; groups
    // are few (a construct subtree) and results are merged by timestamp.
    let mut all_events = Vec::new();
    let mut next_token: Option<String> = None;
    // For follow-up pages we only continue the first group (the common
    // single-group case). Multi-group pagination re-queries all groups with
    // the caller's limit; the CLI passes per-group tokens via repeated calls
    // when it needs exact paging.
    let groups = if query.next_token.is_some() && query.log_groups.len() == 1 {
        &query.log_groups[..1]
    } else {
        &query.log_groups[..]
    };
    for group in groups {
        let mut req = client.filter_log_events().log_group_name(group);
        if let Some(start) = query.start_ms {
            req = req.start_time(start);
        }
        if let Some(end) = query.end_ms {
            req = req.end_time(end);
        }
        if let Some(pattern) = &query.filter_pattern
            && !pattern.is_empty()
        {
            req = req.filter_pattern(pattern);
        }
        if let Some(limit) = query.limit {
            req = req.limit(limit);
        }
        if let Some(token) = &query.next_token
            && query.log_groups.len() == 1
        {
            req = req.next_token(token);
        }
        let out = req.send().await.map_err(map_aws_err)?;
        next_token = out.next_token().map(str::to_string).or(next_token);
        for e in out.events() {
            all_events.push(LogEvent {
                timestamp_ms: e.timestamp().unwrap_or(0),
                message: e.message().unwrap_or("").to_string(),
                log_group: group.clone(),
                log_stream: e.log_stream_name().unwrap_or("").to_string(),
                event_id: e.event_id().map(str::to_string),
            });
        }
    }
    all_events.sort_by_key(|e| e.timestamp_ms);
    if let Some(limit) = query.limit {
        all_events.truncate(limit.max(0) as usize);
    }
    Ok(LogsPage {
        events: all_events,
        next_token,
    })
}

/// Verify log groups exist via `DescribeLogGroups` (FR-9).
/// Returns the subset that was **not** found.
pub async fn find_missing_log_groups(
    client: &aws_sdk_cloudwatchlogs::Client,
    groups: &[String],
) -> Result<Vec<String>, ObsError> {
    let mut missing = Vec::new();
    for group in groups {
        let out = client
            .describe_log_groups()
            .log_group_name_pattern(group)
            .limit(5)
            .send()
            .await
            .map_err(map_aws_err)?;
        let found = out
            .log_groups()
            .iter()
            .any(|g| g.log_group_name().unwrap_or("") == group);
        if !found {
            missing.push(group.clone());
        }
    }
    Ok(missing)
}

// ---------------------------------------------------------------------------
// Logs Insights (explicit opt-in only, FR-13)
// ---------------------------------------------------------------------------

/// One Insights result row as field → value.
pub type InsightsRow = HashMap<String, String>;

/// Run a Logs Insights query and wait for completion (polling).
pub async fn run_insights_query(
    client: &aws_sdk_cloudwatchlogs::Client,
    log_groups: &[String],
    query_string: &str,
    start_ms: i64,
    end_ms: i64,
    limit: Option<i32>,
) -> Result<Vec<InsightsRow>, ObsError> {
    let mut req = client
        .start_query()
        .set_log_group_names(Some(log_groups.to_vec()))
        .query_string(query_string)
        .start_time(start_ms)
        .end_time(end_ms);
    if let Some(l) = limit {
        req = req.limit(l);
    }
    let start_out = req.send().await.map_err(map_aws_err)?;
    let query_id = start_out.query_id().unwrap_or("").to_string();
    if query_id.is_empty() {
        return Err(ObsError::Aws("StartQuery returned no query id".to_string()));
    }
    for _ in 0..60 {
        let out = client
            .get_query_results()
            .query_id(&query_id)
            .send()
            .await
            .map_err(map_aws_err)?;
        match out.status() {
            Some(aws_sdk_cloudwatchlogs::types::QueryStatus::Complete) => {
                let mut rows = Vec::new();
                for row in out.results() {
                    let mut map = HashMap::new();
                    for field in row {
                        map.insert(
                            field.field().unwrap_or("").to_string(),
                            field.value().unwrap_or("").to_string(),
                        );
                    }
                    rows.push(map);
                }
                return Ok(rows);
            }
            Some(
                aws_sdk_cloudwatchlogs::types::QueryStatus::Failed
                | aws_sdk_cloudwatchlogs::types::QueryStatus::Cancelled
                | aws_sdk_cloudwatchlogs::types::QueryStatus::Timeout,
            ) => {
                return Err(ObsError::InsightsFailed(format!(
                    "query {query_id} ended with status {:?}",
                    out.status()
                )));
            }
            // Scheduled / Running / Unknown → keep polling.
            _ => {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
    }
    Err(ObsError::InsightsFailed(format!(
        "query {query_id} did not complete in time"
    )))
}

// ---------------------------------------------------------------------------
// X-Ray
// ---------------------------------------------------------------------------

/// A parsed X-Ray segment document.
#[derive(Debug, Clone, Serialize)]
pub struct XRaySegment {
    pub id: String,
    pub name: String,
    pub start_ms: i64,
    pub end_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    /// Full segment document (parsed JSON).
    pub document: serde_json::Value,
}

/// A full X-Ray trace.
#[derive(Debug, Clone, Serialize)]
pub struct XRayTrace {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    pub segments: Vec<XRaySegment>,
}

/// A trace summary for a service + window (FR-14).
#[derive(Debug, Clone, Serialize)]
pub struct TraceSummary {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    pub has_fault: bool,
    pub has_error: bool,
}

fn segment_from_document(id: String, document: &str) -> XRaySegment {
    let parsed: serde_json::Value =
        serde_json::from_str(document).unwrap_or(serde_json::Value::Null);
    let name = parsed
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let origin = parsed
        .get("origin")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    // Segment times are epoch seconds (float).
    let to_ms = |v: Option<&serde_json::Value>| -> i64 {
        v.and_then(|x| x.as_f64()).map(|s| (s * 1000.0) as i64).unwrap_or(0)
    };
    XRaySegment {
        id,
        name,
        start_ms: to_ms(parsed.get("start_time")),
        end_ms: to_ms(parsed.get("end_time")),
        origin,
        document: parsed,
    }
}

/// Fetch one full trace by ID (`BatchGetTraces`).
pub async fn get_trace(
    client: &aws_sdk_xray::Client,
    trace_id: &str,
) -> Result<XRayTrace, ObsError> {
    let out = client
        .batch_get_traces()
        .trace_ids(trace_id)
        .send()
        .await
        .map_err(map_aws_err)?;
    let trace = out.traces().iter().next().ok_or_else(|| {
        let unprocessed = !out.unprocessed_trace_ids().is_empty();
        if unprocessed {
            ObsError::Aws(format!("trace {trace_id} not yet available (unprocessed)"))
        } else {
            ObsError::TraceNotFound(trace_id.to_string())
        }
    })?;
    let mut segments = Vec::new();
    for seg in trace.segments() {
        segments.push(segment_from_document(
            seg.id().unwrap_or("").to_string(),
            seg.document().unwrap_or("{}"),
        ));
    }
    segments.sort_by_key(|s| s.start_ms);
    Ok(XRayTrace {
        id: trace.id().unwrap_or(trace_id).to_string(),
        duration_secs: trace.duration(),
        segments,
    })
}

/// Trace window for summary queries.
#[derive(Debug, Clone)]
pub struct SummaryQuery {
    pub start_ms: i64,
    pub end_ms: i64,
    pub filter_expression: Option<String>,
    pub limit: Option<i32>,
    pub next_token: Option<String>,
}

/// Fetch trace summaries for a service + window (`GetTraceSummaries`).
pub async fn get_trace_summaries(
    client: &aws_sdk_xray::Client,
    query: &SummaryQuery,
) -> Result<(Vec<TraceSummary>, Option<String>), ObsError> {
    use aws_smithy_types::DateTime;
    let mut req = client
        .get_trace_summaries()
        .start_time(DateTime::from_secs(query.start_ms / 1000))
        .end_time(DateTime::from_secs(query.end_ms / 1000));
    if let Some(expr) = &query.filter_expression
        && !expr.is_empty()
    {
        req = req.filter_expression(expr);
    }
    if let Some(token) = &query.next_token {
        req = req.next_token(token);
    }
    let out = req.send().await.map_err(map_aws_err)?;
    let mut summaries: Vec<TraceSummary> = out
        .trace_summaries()
        .iter()
        .map(|s| TraceSummary {
            id: s.id().unwrap_or("").to_string(),
            duration_secs: s.duration(),
            has_fault: s.has_fault().unwrap_or(false),
            has_error: s.has_error().unwrap_or(false),
        })
        .filter(|s| !s.id.is_empty())
        .collect();
    if let Some(limit) = query.limit {
        summaries.truncate(limit.max(0) as usize);
    }
    Ok((summaries, out.next_token().map(str::to_string)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_back_durations() {
        assert_eq!(parse_back_duration("30s").unwrap(), Duration::from_secs(30));
        assert_eq!(parse_back_duration("15m").unwrap(), Duration::from_secs(900));
        assert_eq!(parse_back_duration("3h").unwrap(), Duration::from_secs(10_800));
        assert_eq!(parse_back_duration("2d").unwrap(), Duration::from_secs(172_800));
        assert_eq!(
            parse_back_duration("1w").unwrap(),
            Duration::from_secs(604_800)
        );
        assert!(parse_back_duration("bogus").is_err());
        assert!(parse_back_duration("10fortnights").is_err());
    }

    #[test]
    fn parses_time_args() {
        // Epoch seconds → millis.
        assert_eq!(parse_time_arg("1727740800").unwrap(), 1_727_740_800_000);
        // Epoch millis pass through.
        assert_eq!(
            parse_time_arg("1727740800123").unwrap(),
            1_727_740_800_123
        );
        // RFC 3339.
        assert_eq!(
            parse_time_arg("2024-10-01T00:00:00Z").unwrap(),
            1_727_740_800_000
        );
        // Date only.
        assert_eq!(parse_time_arg("2024-10-01").unwrap(), 1_727_740_800_000);
        assert!(parse_time_arg("not-a-time").is_err());
    }

    #[test]
    fn resolves_windows() {
        let now = 1_727_748_000_000;
        // Default back 1h.
        let (start, end) = resolve_time_window(None, None, None, now).unwrap();
        assert_eq!(end, Some(now));
        assert_eq!(start, Some(now - 3_600_000));
        // Explicit back.
        let (start, _) = resolve_time_window(Some("30m"), None, None, now).unwrap();
        assert_eq!(start, Some(now - 1_800_000));
        // Start wins over back.
        let (start, end) = resolve_time_window(
            Some("30m"),
            Some("2024-10-01T00:00:00Z"),
            None,
            now,
        )
        .unwrap();
        assert_eq!(start, Some(1_727_740_800_000));
        assert_eq!(end, Some(now));
        // Start after end errors.
        assert!(
            resolve_time_window(
                None,
                Some("2024-10-02T00:00:00Z"),
                Some("2024-10-01T00:00:00Z"),
                now
            )
            .is_err()
        );
    }

    #[test]
    fn parses_segment_documents() {
        let seg = segment_from_document(
            "abc".to_string(),
            r#"{"name":"my-fn","start_time":1727740800.0,"end_time":1727740801.5,"origin":"AWS::Lambda::Function"}"#,
        );
        assert_eq!(seg.name, "my-fn");
        assert_eq!(seg.origin.as_deref(), Some("AWS::Lambda::Function"));
        assert_eq!(seg.start_ms, 1_727_740_800_000);
        assert_eq!(seg.end_ms, 1_727_740_801_500);
    }

    #[test]
    fn handles_malformed_segment_documents() {
        let seg = segment_from_document("x".to_string(), "not json");
        assert_eq!(seg.name, "");
        assert_eq!(seg.origin, None);
    }
}
