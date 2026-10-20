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

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ObsError {
    #[error("aws error: {0}")]
    Aws(String),
    #[error("aws profile {name:?} not found in ~/.aws/config or ~/.aws/credentials{hint}")]
    ProfileNotFound { name: String, hint: String },
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
// Local profiles (pure file parsing, no AWS)
// ---------------------------------------------------------------------------

/// An AWS profile found on this machine.
#[derive(Debug, Clone, Serialize)]
pub struct AwsProfile {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Which file it came from: `config` or `credentials`.
    pub source: String,
}

fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from))
}

/// Path to `~/.aws/config` (or `$AWS_CONFIG_FILE`).
pub fn aws_config_path() -> std::path::PathBuf {
    std::env::var("AWS_CONFIG_FILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".aws")
                .join("config")
        })
}

/// Path to `~/.aws/credentials` (or `$AWS_SHARED_CREDENTIALS_FILE`).
pub fn aws_credentials_path() -> std::path::PathBuf {
    std::env::var("AWS_SHARED_CREDENTIALS_FILE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".aws")
                .join("credentials")
        })
}

/// Minimal INI parse: `(section, {key: value})`. Good enough for AWS files.
fn parse_ini_sections(text: &str) -> Vec<(String, HashMap<String, String>)> {
    let mut out = Vec::new();
    let mut current: Option<(String, HashMap<String, String>)> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            if let Some(section) = current.take() {
                out.push(section);
            }
            current = Some((line[1..line.len() - 1].trim().to_string(), HashMap::new()));
        } else if let Some((_, map)) = current.as_mut() {
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }
    }
    if let Some(section) = current.take() {
        out.push(section);
    }
    out
}

fn profiles_from_files(
    config_text: Option<&str>,
    creds_text: Option<&str>,
) -> Vec<AwsProfile> {
    let mut by_name: HashMap<String, AwsProfile> = HashMap::new();
    // Credentials first (names only), config second (adds regions, wins ties).
    if let Some(text) = creds_text {
        for (section, _) in parse_ini_sections(text) {
            by_name.entry(section.clone()).or_insert(AwsProfile {
                name: section,
                region: None,
                source: "credentials".to_string(),
            });
        }
    }
    if let Some(text) = config_text {
        for (section, keys) in parse_ini_sections(text) {
            let name = section
                .strip_prefix("profile ")
                .unwrap_or(&section)
                .trim()
                .to_string();
            let region = keys.get("region").cloned();
            by_name
                .entry(name.clone())
                .and_modify(|p| {
                    if p.region.is_none() {
                        p.region = region.clone();
                    }
                    p.source = "config".to_string();
                })
                .or_insert(AwsProfile {
                    name,
                    region,
                    source: "config".to_string(),
                });
        }
    }
    let mut out: Vec<AwsProfile> = by_name.into_values().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Profiles from the standard local files. Never touches the network.
pub fn available_profiles() -> Vec<AwsProfile> {
    let config = std::fs::read_to_string(aws_config_path()).ok();
    let creds = std::fs::read_to_string(aws_credentials_path()).ok();
    profiles_from_files(config.as_deref(), creds.as_deref())
}

/// Fail fast with a helpful message when `--profile` names nothing on this
/// machine. Profile names are case-sensitive; a wrong-case name otherwise
/// dies deep inside the SDK as an opaque "dispatch failure".
pub fn validate_profile(name: &str) -> Result<(), ObsError> {
    let all = available_profiles();
    if all.iter().any(|p| p.name == name) {
        return Ok(());
    }
    let suggestion = all
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
        .map(|p| format!(" Did you mean {:?}?", p.name))
        .unwrap_or_default();
    let available = if all.is_empty() {
        "none found — run `aws configure` or `aws sso login`".to_string()
    } else {
        all.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ")
    };
    Err(ObsError::ProfileNotFound {
        name: name.to_string(),
        hint: format!(". Available: {available}.{suggestion}"),
    })
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
}

/// Pagination state for one log group.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupCursor {
    pub log_group: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
    /// No further pages for this group (window exhausted).
    #[serde(default)]
    pub done: bool,
}

/// Per-group pagination cursor; pass back into `filter_log_events` to fetch
/// the next page across all groups (FR-12 multi-group paging).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsCursor {
    pub groups: Vec<GroupCursor>,
}

impl LogsCursor {
    pub fn token_for(&self, group: &str) -> Option<&str> {
        self.groups
            .iter()
            .find(|g| g.log_group == group)
            .and_then(|g| g.next_token.as_deref())
    }

    pub fn is_done(&self, group: &str) -> bool {
        self.groups
            .iter()
            .find(|g| g.log_group == group)
            .is_some_and(|g| g.done)
    }

    pub fn has_more(&self) -> bool {
        self.groups.iter().any(|g| !g.done)
    }
}

/// One page of `FilterLogEvents` results plus the cursor for the next page.
#[derive(Debug, Clone, Default)]
pub struct LogsPage {
    pub events: Vec<LogEvent>,
    pub next_cursor: Option<LogsCursor>,
}

fn compact_one_line(s: String, max: usize) -> String {
    let one_line: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() > max {
        format!("{}…", one_line.chars().take(max).collect::<String>())
    } else {
        one_line
    }
}

fn map_aws_err<E: std::fmt::Display + std::fmt::Debug>(e: E) -> ObsError {
    // The SDK's Display for transport/service failures is often a bare
    // "dispatch failure" / "service error" with the cause only in Debug.
    // Prefer the modeled service message (e.g. "The specified log group
    // does not exist.") so banners say something actionable.
    const GENERIC: &[&str] = &[
        "service error",
        "dispatch failure",
        "timeout error",
        "connection error",
    ];
    let display = e.to_string();
    if GENERIC.iter().any(|g| display.trim().eq_ignore_ascii_case(g)) {
        let debug = format!("{e:?}");
        if let Some(msg) = modeled_message(&debug) {
            if msg.contains("log group does not exist") {
                return ObsError::Aws(format!(
                    "{msg} — the group isn't in this region. Deploy the stack, or check profile/region."
                ));
            }
            return ObsError::Aws(msg);
        }
        ObsError::Aws(compact_one_line(format!("{display} — {debug}"), 600))
    } else {
        ObsError::Aws(display)
    }
}

/// Pull the modeled service message out of a Debug-formatted SDK error.
/// Debug looks like: `... message: Some("The specified log group does not
/// exist."), ...`. Returns None when there is no modeled message.
fn modeled_message(debug: &str) -> Option<String> {
    const KEY: &str = "message: Some(\"";
    let from = debug.find(KEY)? + KEY.len();
    let mut out = String::new();
    let mut escaped = false;
    for c in debug[from..].chars() {
        if escaped {
            out.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            break;
        } else {
            out.push(c);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Run one `FilterLogEvents` request per group (fine-grained per-group
/// pagination), merging the results by timestamp. Pass the returned
/// `next_cursor` back in to continue paging; `None` means the window is
/// exhausted for every group.
pub async fn filter_log_events(
    client: &aws_sdk_cloudwatchlogs::Client,
    query: &LogsQuery,
    cursor: Option<&LogsCursor>,
) -> Result<LogsPage, ObsError> {
    if query.log_groups.is_empty() {
        return Ok(LogsPage::default());
    }
    let mut all_events = Vec::new();
    let mut group_cursors: Vec<GroupCursor> = Vec::with_capacity(query.log_groups.len());
    for group in &query.log_groups {
        if cursor.is_some_and(|c| c.is_done(group)) {
            group_cursors.push(GroupCursor {
                log_group: group.clone(),
                next_token: None,
                done: true,
            });
            continue;
        }
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
        if let Some(token) = cursor.and_then(|c| c.token_for(group)) {
            req = req.next_token(token);
        }
        let out = req.send().await.map_err(map_aws_err)?;
        let next_token = out.next_token().map(str::to_string);
        group_cursors.push(GroupCursor {
            log_group: group.clone(),
            done: next_token.is_none(),
            next_token,
        });
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
    let next_cursor = if group_cursors.iter().any(|g| !g.done) {
        Some(LogsCursor {
            groups: group_cursors,
        })
    } else {
        None
    };
    Ok(LogsPage {
        events: all_events,
        next_cursor,
    })
}

/// Stream CloudWatch Logs live via `StartLiveTail` (FR-10, FR-21).
///
/// Feeds each session update to `on_batch` (events arrive pre-batched from
/// the service). Returns when the stream closes or the session hits the
/// service's 3-hour cap (`SessionTimeout`); callers may reconnect. Dropping
/// the future cancels the stream.
///
/// Note: the service allows 5 concurrent sessions per account/region.
pub async fn live_tail_stream<F>(
    client: &aws_sdk_cloudwatchlogs::Client,
    log_groups: &[String],
    filter_pattern: Option<&str>,
    mut on_batch: F,
) -> Result<(), ObsError>
where
    F: FnMut(Vec<LogEvent>),
{
    if log_groups.is_empty() {
        return Ok(());
    }
    let mut req = client.start_live_tail();
    for group in log_groups {
        req = req.log_group_identifiers(group);
    }
    if let Some(pattern) = filter_pattern
        && !pattern.is_empty()
    {
        req = req.log_event_filter_pattern(pattern);
    }
    let out = req.send().await.map_err(map_aws_err)?;
    let mut stream = out.response_stream;
    loop {
        match stream.recv().await {
            Ok(Some(
                aws_sdk_cloudwatchlogs::types::StartLiveTailResponseStream::SessionUpdate(update),
            )) => {
                let events: Vec<LogEvent> = update
                    .session_results()
                    .iter()
                    .map(|e| LogEvent {
                        timestamp_ms: e.timestamp().unwrap_or(0),
                        message: e.message().unwrap_or("").to_string(),
                        log_group: e.log_group_identifier().unwrap_or("").to_string(),
                        log_stream: e.log_stream_name().unwrap_or("").to_string(),
                        event_id: None,
                    })
                    .collect();
                if !events.is_empty() {
                    on_batch(events);
                }
            }
            // 3-hour session cap arrives as a stream error; treat as a normal
            // end so the caller can reconnect.
            Ok(Some(_)) => {}
            Ok(None) => return Ok(()),
            Err(e) => {
                if let aws_sdk_cloudwatchlogs::error::SdkError::ServiceError(se) = &e
                    && se.err().is_session_timeout_exception()
                {
                    return Ok(());
                }
                return Err(map_aws_err(e));
            }
        }
    }
}

/// Cheap credential/region probe: one `DescribeLogGroups` page.
pub async fn probe_logs_access(
    client: &aws_sdk_cloudwatchlogs::Client,
) -> Result<(), ObsError> {
    client
        .describe_log_groups()
        .limit(1)
        .send()
        .await
        .map_err(map_aws_err)?;
    Ok(())
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

fn is_false(b: &bool) -> bool {
    !*b
}

/// A parsed X-Ray subsegment, recursively nested.
#[derive(Debug, Clone, Default, Serialize)]
pub struct XRaySubsegment {
    pub id: String,
    pub name: String,
    pub start_ms: i64,
    pub end_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aws_operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<i32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub error: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub fault: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub throttle: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub subsegments: Vec<XRaySubsegment>,
}

/// A parsed X-Ray segment document.
#[derive(Debug, Clone, Serialize)]
pub struct XRaySegment {
    pub id: String,
    pub name: String,
    pub start_ms: i64,
    pub end_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub error: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub fault: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub throttle: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub subsegments: Vec<XRaySubsegment>,
    /// Full segment document (parsed JSON).
    pub document: serde_json::Value,
}

/// Guard against cyclic/degenerate documents; X-Ray SDKs nest only a handful
/// of levels in practice.
const MAX_SUBSEGMENT_DEPTH: u32 = 32;
const MAX_CAUSE_LEN: usize = 600;

fn value_to_ms(v: Option<&serde_json::Value>) -> i64 {
    v.and_then(|x| x.as_f64())
        .map(|s| (s * 1000.0) as i64)
        .unwrap_or(0)
}

fn value_to_http_status(v: &serde_json::Value) -> Option<i32> {
    let status = v.get("http")?.get("response")?.get("status")?;
    if let Some(n) = status.as_i64() {
        return Some(n as i32);
    }
    status.as_str().and_then(|s| s.parse().ok())
}

fn value_to_cause(v: &serde_json::Value) -> Option<String> {
    let cause = v.get("cause")?;
    let msg = if let Some(s) = cause.as_str() {
        s.to_string()
    } else {
        cause
            .get("exceptions")
            .and_then(|e| e.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| {
                let ty = first.get("type").and_then(|t| t.as_str());
                let message = first.get("message").and_then(|m| m.as_str());
                match (ty, message) {
                    (Some(t), Some(m)) => Some(format!("{t}: {m}")),
                    (None, Some(m)) => Some(m.to_string()),
                    (Some(t), None) => Some(t.to_string()),
                    (None, None) => None,
                }
            })
            .or_else(|| {
                cause
                    .get("exception")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })?
    };
    if msg.is_empty() {
        return None;
    }
    Some(compact_one_line(msg, MAX_CAUSE_LEN))
}

fn subsegment_from_value(v: &serde_json::Value, depth: u32) -> Option<XRaySubsegment> {
    if depth > MAX_SUBSEGMENT_DEPTH {
        return None;
    }
    Some(XRaySubsegment {
        id: v.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        name: v
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        start_ms: value_to_ms(v.get("start_time")),
        end_ms: value_to_ms(v.get("end_time")),
        origin: v.get("origin").and_then(|x| x.as_str()).map(str::to_string),
        namespace: v
            .get("namespace")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        aws_operation: v
            .get("aws")
            .and_then(|a| a.get("operation"))
            .and_then(|x| x.as_str())
            .map(str::to_string),
        http_status: value_to_http_status(v),
        error: v.get("error").and_then(|x| x.as_bool()).unwrap_or(false),
        fault: v.get("fault").and_then(|x| x.as_bool()).unwrap_or(false),
        throttle: v.get("throttle").and_then(|x| x.as_bool()).unwrap_or(false),
        cause: value_to_cause(v),
        subsegments: parse_subsegments(v.get("subsegments"), depth + 1),
    })
}

fn parse_subsegments(v: Option<&serde_json::Value>, depth: u32) -> Vec<XRaySubsegment> {
    if depth > MAX_SUBSEGMENT_DEPTH {
        return Vec::new();
    }
    v.and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|s| subsegment_from_value(s, depth))
                .collect()
        })
        .unwrap_or_default()
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
    XRaySegment {
        id,
        name,
        start_ms: value_to_ms(parsed.get("start_time")),
        end_ms: value_to_ms(parsed.get("end_time")),
        origin,
        error: parsed
            .get("error")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        fault: parsed
            .get("fault")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        throttle: parsed
            .get("throttle")
            .and_then(|x| x.as_bool())
            .unwrap_or(false),
        http_status: value_to_http_status(&parsed),
        cause: value_to_cause(&parsed),
        subsegments: parse_subsegments(parsed.get("subsegments"), 0),
        document: parsed,
    }
}

/// Resolve an AWS Lambda request ID to an X-Ray trace ID.
///
/// Strategy: list recent summaries in `[start_ms, end_ms]`, then scan each
/// full trace's segment documents for the request UUID. Lambda START/END
/// lines rarely embed `1-…-…` trace IDs, so this is the correlation path.
pub async fn find_trace_id_for_request_id(
    client: &aws_sdk_xray::Client,
    request_id: &str,
    start_ms: i64,
    end_ms: i64,
) -> Result<Option<String>, ObsError> {
    let rid = request_id.trim();
    if rid.is_empty() {
        return Ok(None);
    }
    // Prefer annotation filters when present; fall back to an unfiltered window.
    let filters = [
        format!(r#"annotation["aws:lambda:requestId"] = "{rid}""#),
        format!(r#"annotation.RequestId = "{rid}""#),
        String::new(),
    ];
    for filter in &filters {
        let has_filter = !filter.is_empty();
        let expr = if has_filter {
            Some(filter.clone())
        } else {
            None
        };
        let summaries = match get_trace_summaries(
            client,
            &SummaryQuery {
                start_ms,
                end_ms,
                filter_expression: expr,
                limit: Some(40),
                next_token: None,
            },
        )
        .await
        {
            Ok((s, _)) => s,
            // Bad annotation filter → try the next strategy.
            Err(ObsError::Aws(_)) if has_filter => continue,
            Err(e) => return Err(e),
        };
        if summaries.is_empty() {
            continue;
        }
        for summary in summaries {
            match get_trace(client, &summary.id).await {
                Ok(trace) => {
                    if trace_mentions_request_id(&trace, rid) {
                        return Ok(Some(trace.id));
                    }
                }
                Err(ObsError::TraceNotFound(_)) => continue,
                Err(ObsError::Aws(msg)) if msg.contains("unprocessed") => continue,
                Err(e) => return Err(e),
            }
        }
        // Annotation filter matched summaries but none contained the id —
        // try the next strategy.
    }
    Ok(None)
}

fn trace_mentions_request_id(trace: &XRayTrace, request_id: &str) -> bool {
    for seg in &trace.segments {
        let dump = seg.document.to_string();
        if dump.contains(request_id) {
            return true;
        }
    }
    false
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
        assert!(!seg.error);
        assert!(seg.subsegments.is_empty());
    }

    #[test]
    fn parses_nested_subsegments() {
        let doc = r#"{
            "name":"my-fn","start_time":1727740800.0,"end_time":1727740801.5,
            "origin":"AWS::Lambda::Function",
            "subsegments":[
                {"id":"sub1","name":"DynamoDB","start_time":1727740800.1,"end_time":1727740800.4,
                 "namespace":"aws","aws":{"operation":"PutItem"},
                 "http":{"response":{"status":400}},"error":true,
                 "cause":{"exceptions":[{"type":"ValidationException","message":"bad item"}]},
                 "subsegments":[
                    {"id":"sub2","name":"OrdersTable","start_time":1727740800.1,"end_time":1727740800.3}
                 ]},
                {"id":"sub3","name":"HTTP GET","start_time":1727740800.5,"end_time":1727740800.9,
                 "http":{"response":{"status":"200"}}}
            ]
        }"#;
        let seg = segment_from_document("root".to_string(), doc);
        assert_eq!(seg.subsegments.len(), 2);
        let db = &seg.subsegments[0];
        assert_eq!(db.name, "DynamoDB");
        assert_eq!(db.aws_operation.as_deref(), Some("PutItem"));
        assert_eq!(db.http_status, Some(400));
        assert!(db.error);
        assert!(!db.fault);
        assert_eq!(db.cause.as_deref(), Some("ValidationException: bad item"));
        assert_eq!(db.start_ms, 1_727_740_800_100);
        assert_eq!(db.subsegments.len(), 1);
        assert_eq!(db.subsegments[0].name, "OrdersTable");
        let http = &seg.subsegments[1];
        assert_eq!(http.http_status, Some(200));
        assert!(!http.error);
    }

    #[test]
    fn parses_segment_error_fields() {
        let doc = r#"{"name":"my-fn","start_time":1.0,"end_time":2.0,
            "fault":true,"cause":"RuntimeError: boom"}"#;
        let seg = segment_from_document("root".to_string(), doc);
        assert!(seg.fault);
        assert!(!seg.error);
        assert_eq!(seg.cause.as_deref(), Some("RuntimeError: boom"));
    }

    #[test]
    fn cursor_tracks_per_group_paging() {
        let cursor = LogsCursor {
            groups: vec![
                GroupCursor {
                    log_group: "/aws/lambda/a".into(),
                    next_token: Some("t1".into()),
                    done: false,
                },
                GroupCursor {
                    log_group: "/aws/lambda/b".into(),
                    next_token: None,
                    done: true,
                },
            ],
        };
        assert_eq!(cursor.token_for("/aws/lambda/a"), Some("t1"));
        assert_eq!(cursor.token_for("/aws/lambda/b"), None);
        assert!(cursor.is_done("/aws/lambda/b"));
        assert!(!cursor.is_done("/aws/lambda/a"));
        assert!(cursor.token_for("/aws/lambda/missing").is_none());
        assert!(cursor.has_more());

        let exhausted = LogsCursor {
            groups: vec![GroupCursor {
                log_group: "/aws/lambda/a".into(),
                next_token: None,
                done: true,
            }],
        };
        assert!(!exhausted.has_more());
        assert!(!exhausted.is_done("/aws/lambda/missing"));
    }

    #[test]
    fn parses_profile_files() {
        let config = "[default]\nregion = us-east-1\n[profile dev-sso]\nregion = eu-west-1\n";
        let creds = "[cw-trace]\naws_access_key_id = X\n[dev-sso]\naws_access_key_id = Y\n";
        let profiles = profiles_from_files(Some(config), Some(creds));
        let names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["cw-trace", "default", "dev-sso"]);
        let dev = profiles.iter().find(|p| p.name == "dev-sso").unwrap();
        assert_eq!(dev.region.as_deref(), Some("eu-west-1"));
        assert_eq!(dev.source, "config");
        let cw = profiles.iter().find(|p| p.name == "cw-trace").unwrap();
        assert_eq!(cw.region, None);
    }

    #[test]
    fn enriches_generic_aws_errors() {
        #[derive(Debug)]
        struct Generic;
        impl std::fmt::Display for Generic {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "dispatch failure")
            }
        }
        let err = map_aws_err(Generic);
        let msg = err.to_string();
        assert!(msg.contains("dispatch failure"), "{msg}");
        assert!(msg.contains("Generic"), "{msg}");

        #[derive(Debug)]
        struct Specific;
        impl std::fmt::Display for Specific {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "The specified log group does not exist.")
            }
        }
        assert_eq!(
            map_aws_err(Specific).to_string(),
            "aws error: The specified log group does not exist."
        );
    }

    #[test]
    fn extracts_modeled_service_message() {
        #[derive(Debug)]
        struct Modeled;
        impl std::fmt::Display for Modeled {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "service error")
            }
        }
        impl Modeled {
            fn debug_str() -> &'static str {
                "ServiceError { source: ResourceNotFoundException { message: Some(\"The specified log group does not exist.\"), meta: {} }, raw: {} }"
            }
        }
        // modeled_message parses the Debug shape directly.
        assert_eq!(
            modeled_message(Modeled::debug_str()),
            Some("The specified log group does not exist.".to_string())
        );
        assert_eq!(modeled_message("no message here"), None);
        // End-to-end through map_aws_err with a Debug impl carrying a message.
        struct WithMsg;
        impl std::fmt::Display for WithMsg {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "service error")
            }
        }
        impl std::fmt::Debug for WithMsg {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", Modeled::debug_str())
            }
        }
        let msg = map_aws_err(WithMsg).to_string();
        assert!(msg.contains("The specified log group does not exist."), "{msg}");
        assert!(!msg.contains("ServiceError"), "{msg}");
        assert!(msg.contains("Deploy the stack"), "{msg}");
    }
}
