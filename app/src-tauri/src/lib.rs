//! Tauri backend for cw-trace (M3).
//!
//! Thin command layer over the workspace core crates (`synth`, `cdk-graph`,
//! `aws-obs`, `correlate`). All commands are read-only against AWS.
//!
//! Streaming tail uses batched `log-batch` events (never one event per line,
//! FR-21).

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub profile: Option<String>,
    pub region: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub timestamp_ms: i64,
    pub timestamp: String,
    pub log_group: String,
    pub log_stream: String,
    pub message: String,
    pub trace_ids: Vec<String>,
    pub lambda_request_ids: Vec<String>,
    pub apigw_request_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsInput {
    pub connection: Connection,
    pub cdk_out: Option<String>,
    pub construct_path: String,
    pub back: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub filter: Option<String>,
    pub limit: Option<i32>,
    /// Continue a previous search ("load newer").
    #[serde(default)]
    pub cursor: Option<aws_obs::LogsCursor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceInput {
    pub connection: Connection,
    pub id: String,
    pub cdk_out: Option<String>,
}

pub struct TailState {
    pub running: Mutex<bool>,
    pub handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

/// Log search results plus the groups that don't exist in CloudWatch.
/// Missing groups are reported, not fatal: the stack may simply not be
/// deployed yet (log groups appear on first invocation).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogsResult {
    pub lines: Vec<LogLine>,
    pub missing: Vec<String>,
    /// Per-group cursor for the next page; absent when the window is drained.
    #[serde(default)]
    pub next_cursor: Option<aws_obs::LogsCursor>,
}

fn format_ts(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| ms.to_string())
}

fn resolve_outdir(cdk_out: Option<&str>) -> Result<std::path::PathBuf, String> {
    if let Some(dir) = cdk_out {
        return Ok(std::path::PathBuf::from(dir));
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    synth::CdkProject::locate(&cwd)
        .map(|p| p.outdir().to_path_buf())
        .map_err(|e| format!("no cdk.json found ({e}); pass cdkOut explicitly"))
}

fn to_log_line(e: &aws_obs::LogEvent) -> LogLine {
    let jump = correlate::extract_jumps(&e.message);
    LogLine {
        timestamp_ms: e.timestamp_ms,
        timestamp: format_ts(e.timestamp_ms),
        log_group: e.log_group.clone(),
        log_stream: e.log_stream.clone(),
        message: e.message.clone(),
        trace_ids: jump.trace_ids,
        lambda_request_ids: jump.lambda_request_ids,
        apigw_request_ids: jump.apigw_request_ids,
    }
}

#[tauri::command]
async fn get_status(connection: Connection) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "profile": connection.profile.unwrap_or("(default chain)".to_string()),
        "region": connection.region.unwrap_or("(default chain)".to_string()),
    }))
}

#[derive(Debug, Clone, Serialize)]
struct GraphPayload {
    nodes: Vec<cdk_graph::ResourceNode>,
    edges: Vec<cdk_graph::ResourceEdge>,
}

#[tauri::command]
async fn get_graph(cdk_out: Option<String>) -> Result<GraphPayload, String> {
    let dir = resolve_outdir(cdk_out.as_deref())?;
    cdk_graph::ResourceGraph::from_cdk_out(&dir)
        .map(|g| GraphPayload {
            nodes: g.nodes().to_vec(),
            edges: g.edges().to_vec(),
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn search_logs(input: LogsInput) -> Result<LogsResult, String> {
    let dir = resolve_outdir(input.cdk_out.as_deref())?;
    let graph =
        cdk_graph::ResourceGraph::from_cdk_out(&dir).map_err(|e| e.to_string())?;
    let needle = input.construct_path.trim().trim_matches('/').to_string();
    let mut groups: Vec<String> = graph
        .nodes()
        .iter()
        .filter(|n| {
            n.construct_path == needle
                || n.construct_path.starts_with(&format!("{needle}/"))
        })
        .filter_map(|n| n.log_group.clone())
        .collect();
    groups.sort();
    groups.dedup();
    if groups.is_empty() {
        return Err(format!("no log groups for construct {:?}", input.construct_path));
    }
    let now = aws_obs::now_ms();
    let (start_ms, end_ms) = aws_obs::resolve_time_window(
        input.back.as_deref(),
        input.start.as_deref(),
        input.end.as_deref(),
        now,
    )
    .map_err(|e| e.to_string())?;
    if let Some(p) = &input.connection.profile {
        aws_obs::validate_profile(p).map_err(|e| e.to_string())?;
    }
    let sdk = aws_obs::load_sdk_config(&aws_obs::AwsOptions::new(
        input.connection.profile.clone(),
        input.connection.region.clone(),
    ))
    .await;
    let client = aws_obs::cloudwatch_client(&sdk);
    // Verify groups exist first: missing ones are reported (undeployed
    // stack / pre-first-invocation), not fatal. Only query what's there.
    let missing = aws_obs::find_missing_log_groups(&client, &groups)
        .await
        .unwrap_or_default();
    let present: Vec<String> = groups
        .into_iter()
        .filter(|g| !missing.contains(g))
        .collect();
    if present.is_empty() {
        return Ok(LogsResult {
            lines: Vec::new(),
            missing,
            next_cursor: None,
        });
    }
    let page = aws_obs::filter_log_events(
        &client,
        &aws_obs::LogsQuery {
            log_groups: present,
            start_ms,
            end_ms,
            filter_pattern: input.filter.clone(),
            limit: Some(input.limit.unwrap_or(100).clamp(1, 10_000)),
        },
        input.cursor.as_ref(),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(LogsResult {
        lines: page.events.iter().map(to_log_line).collect(),
        missing,
        next_cursor: page.next_cursor,
    })
}

fn trace_payload(trace: aws_obs::XRayTrace) -> serde_json::Value {
    let (mut start, mut end) = (i64::MAX, i64::MIN);
    for seg in &trace.segments {
        start = start.min(seg.start_ms);
        end = end.max(seg.end_ms);
    }
    if start == i64::MAX {
        let now = aws_obs::now_ms();
        start = now - 60_000;
        end = now;
    }
    let (suggest_start, suggest_end) = correlate::suggest_log_window(start, end, 300);
    serde_json::json!({
        "trace": trace,
        "suggestedLogWindow": {
            "startMs": suggest_start,
            "endMs": suggest_end,
            "start": format_ts(suggest_start),
            "end": format_ts(suggest_end),
        },
    })
}

#[tauri::command]
async fn fetch_trace(input: TraceInput) -> Result<serde_json::Value, String> {
    if !correlate::is_trace_id(&input.id) {
        return Err(format!("invalid X-Ray trace ID {:?}", input.id));
    }
    if let Some(p) = &input.connection.profile {
        aws_obs::validate_profile(p).map_err(|e| e.to_string())?;
    }
    let sdk = aws_obs::load_sdk_config(&aws_obs::AwsOptions::new(
        input.connection.profile.clone(),
        input.connection.region.clone(),
    ))
    .await;
    let trace = aws_obs::get_trace(&aws_obs::xray_client(&sdk), &input.id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(trace_payload(trace))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveRequestInput {
    pub connection: Connection,
    pub request_id: String,
    /// Epoch ms of the log line (window is ±15m around this).
    pub around_ms: Option<i64>,
    pub back: Option<String>,
}

/// Resolve a Lambda RequestId to a full X-Ray trace (FR-15/FR-16).
#[tauri::command]
async fn resolve_request_trace(input: ResolveRequestInput) -> Result<serde_json::Value, String> {
    let rid = input.request_id.trim();
    if rid.is_empty() {
        return Err("empty request id".to_string());
    }
    if let Some(p) = &input.connection.profile {
        aws_obs::validate_profile(p).map_err(|e| e.to_string())?;
    }
    let now = aws_obs::now_ms();
    let (start_ms, end_ms) = if let Some(around) = input.around_ms {
        let pad = 15 * 60 * 1000;
        (around.saturating_sub(pad), around.saturating_add(pad).min(now))
    } else {
        let (s, e) = aws_obs::resolve_time_window(
            input.back.as_deref().or(Some("1h")),
            None,
            None,
            now,
        )
        .map_err(|e| e.to_string())?;
        (s.unwrap_or(now - 3_600_000), e.unwrap_or(now))
    };
    let sdk = aws_obs::load_sdk_config(&aws_obs::AwsOptions::new(
        input.connection.profile.clone(),
        input.connection.region.clone(),
    ))
    .await;
    let xray = aws_obs::xray_client(&sdk);
    let found = aws_obs::find_trace_id_for_request_id(&xray, rid, start_ms, end_ms)
        .await
        .map_err(|e| e.to_string())?;
    let Some(trace_id) = found else {
        return Err(format!(
            "no X-Ray trace found for request id {rid} in the last window — tracing may be off, or the window is too narrow"
        ));
    };
    let trace = aws_obs::get_trace(&xray, &trace_id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(trace_payload(trace))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyInput {
    pub connection: Connection,
    pub cdk_out: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    pub missing: Vec<String>,
    pub checked: Vec<String>,
}

/// DescribeLogGroups for every mapped group in the graph (FR-9).
#[tauri::command]
async fn verify_log_groups(input: VerifyInput) -> Result<VerifyResult, String> {
    let dir = resolve_outdir(input.cdk_out.as_deref())?;
    let graph =
        cdk_graph::ResourceGraph::from_cdk_out(&dir).map_err(|e| e.to_string())?;
    let mut checked: Vec<String> = graph
        .log_groups()
        .filter_map(|n| n.log_group.clone())
        .collect();
    checked.sort();
    checked.dedup();
    if checked.is_empty() {
        return Ok(VerifyResult {
            missing: Vec::new(),
            checked,
        });
    }
    if let Some(p) = &input.connection.profile {
        aws_obs::validate_profile(p).map_err(|e| e.to_string())?;
    }
    let sdk = aws_obs::load_sdk_config(&aws_obs::AwsOptions::new(
        input.connection.profile.clone(),
        input.connection.region.clone(),
    ))
    .await;
    let missing = aws_obs::find_missing_log_groups(&aws_obs::cloudwatch_client(&sdk), &checked)
        .await
        .map_err(|e| e.to_string())?;
    Ok(VerifyResult { missing, checked })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthResultDto {
    pub outdir: String,
    pub stale: bool,
}

/// Run `cdk synth` for a project directory (FR-3).
#[tauri::command]
async fn run_synth(project_dir: String) -> Result<SynthResultDto, String> {
    let start = std::path::PathBuf::from(&project_dir);
    let project = synth::CdkProject::locate(&start).map_err(|e| e.to_string())?;
    let outdir = project.outdir().display().to_string();
    tokio::task::spawn_blocking(move || {
        project
            .synthesize(std::time::Duration::from_secs(180))
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(SynthResultDto {
        outdir,
        stale: false,
    })
}

/// Probe AWS credentials for a profile/region (stage sheet "Test connection").
#[tauri::command]
async fn test_connection(connection: Connection) -> Result<serde_json::Value, String> {
    if let Some(p) = &connection.profile {
        aws_obs::validate_profile(p).map_err(|e| e.to_string())?;
    }
    let sdk = aws_obs::load_sdk_config(&aws_obs::AwsOptions::new(
        connection.profile.clone(),
        connection.region.clone(),
    ))
    .await;
    aws_obs::probe_logs_access(&aws_obs::cloudwatch_client(&sdk))
        .await
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "profile": connection.profile.unwrap_or_else(|| "(default chain)".into()),
        "region": connection.region.unwrap_or_else(|| "(default chain)".into()),
    }))
}

fn log_groups_for_construct(
    graph: &cdk_graph::ResourceGraph,
    construct_path: &str,
) -> Vec<String> {
    let needle = construct_path.trim().trim_matches('/');
    let mut groups: Vec<String> = graph
        .nodes()
        .iter()
        .filter(|n| {
            n.construct_path == needle
                || n.construct_path.starts_with(&format!("{needle}/"))
        })
        .filter_map(|n| n.log_group.clone())
        .collect();
    groups.sort();
    groups.dedup();
    groups
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InsightsInput {
    pub connection: Connection,
    pub cdk_out: Option<String>,
    pub construct_path: String,
    pub query: String,
    pub back: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub limit: Option<i32>,
}

/// Explicit Logs Insights run (FR-13) — never the default path.
#[tauri::command]
async fn run_insights(input: InsightsInput) -> Result<Vec<serde_json::Value>, String> {
    let dir = resolve_outdir(input.cdk_out.as_deref())?;
    let graph =
        cdk_graph::ResourceGraph::from_cdk_out(&dir).map_err(|e| e.to_string())?;
    let groups = log_groups_for_construct(&graph, &input.construct_path);
    if groups.is_empty() {
        return Err(format!(
            "no log groups for construct {:?}",
            input.construct_path
        ));
    }
    let now = aws_obs::now_ms();
    let (start_ms, end_ms) = aws_obs::resolve_time_window(
        input.back.as_deref(),
        input.start.as_deref(),
        input.end.as_deref(),
        now,
    )
    .map_err(|e| e.to_string())?;
    let (s, e) = match (start_ms, end_ms) {
        (Some(s), Some(e)) => (s, e),
        _ => return Err("Insights requires a bounded time window".into()),
    };
    if let Some(p) = &input.connection.profile {
        aws_obs::validate_profile(p).map_err(|e| e.to_string())?;
    }
    let sdk = aws_obs::load_sdk_config(&aws_obs::AwsOptions::new(
        input.connection.profile.clone(),
        input.connection.region.clone(),
    ))
    .await;
    let rows = aws_obs::run_insights_query(
        &aws_obs::cloudwatch_client(&sdk),
        &groups,
        &input.query,
        s,
        e,
        Some(input.limit.unwrap_or(50).clamp(1, 1000)),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|m| serde_json::to_value(m).unwrap_or_default())
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummariesInput {
    pub connection: Connection,
    pub back: Option<String>,
    pub start: Option<String>,
    pub end: Option<String>,
    pub filter: Option<String>,
    pub service: Option<String>,
    pub limit: Option<i32>,
}

/// List recent X-Ray traces for a window (FR-14).
#[tauri::command]
async fn list_trace_summaries(
    input: SummariesInput,
) -> Result<Vec<aws_obs::TraceSummary>, String> {
    if let Some(p) = &input.connection.profile {
        aws_obs::validate_profile(p).map_err(|e| e.to_string())?;
    }
    let now = aws_obs::now_ms();
    let (start_ms, end_ms) = aws_obs::resolve_time_window(
        input.back.as_deref().or(Some("1h")),
        input.start.as_deref(),
        input.end.as_deref(),
        now,
    )
    .map_err(|e| e.to_string())?;
    let (s, e) = match (start_ms, end_ms) {
        (Some(s), Some(e)) => (s, e),
        _ => return Err("trace summaries require a bounded time window".into()),
    };
    let filter_expression = match (&input.filter, &input.service) {
        (Some(f), _) if !f.is_empty() => Some(f.clone()),
        (_, Some(svc)) if !svc.is_empty() => Some(format!("service(\"{svc}\")")),
        _ => None,
    };
    let sdk = aws_obs::load_sdk_config(&aws_obs::AwsOptions::new(
        input.connection.profile.clone(),
        input.connection.region.clone(),
    ))
    .await;
    let (summaries, _) = aws_obs::get_trace_summaries(
        &aws_obs::xray_client(&sdk),
        &aws_obs::SummaryQuery {
            start_ms: s,
            end_ms: e,
            filter_expression,
            limit: Some(input.limit.unwrap_or(25).clamp(1, 100)),
            next_token: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(summaries)
}

/// Reveal a path in Finder / file manager.
#[tauri::command]
fn reveal_in_finder(path: String) -> Result<(), String> {
    let p = std::path::PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("path not found: {path}"));
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R", &path])
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .args(["/select,", &path])
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let parent = p.parent().unwrap_or(&p);
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Open an https URL in the default browser (console deep links).
#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only http(s) URLs allowed".into());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Copy a shell command and open Terminal (macOS) / run guidance elsewhere.
#[tauri::command]
fn open_terminal_with(command: String) -> Result<(), String> {
    let cmd = command.trim();
    if cmd.is_empty() {
        return Err("empty command".into());
    }
    // Block obvious injection — allow aws SSO / profile login only.
    if !(cmd.starts_with("aws sso login") || cmd.starts_with("aws configure")) {
        return Err("only aws sso login / aws configure allowed".into());
    }
    #[cfg(target_os = "macos")]
    {
        let escaped = cmd.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "tell application \"Terminal\" to do script \"{escaped}\""
        );
        std::process::Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map_err(|e| e.to_string())?;
        let _ = std::process::Command::new("osascript")
            .args(["-e", "tell application \"Terminal\" to activate"])
            .spawn();
        return Ok(());
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err(format!(
            "open Terminal is macOS-only — run this yourself: {cmd}"
        ))
    }
}

/// Start a batched tail: polls CloudWatch and emits `log-batch` events with
/// up to `limit` lines each (FR-21: never one event per line).
#[tauri::command]
async fn start_tail(
    app: AppHandle,
    input: LogsInput,
    interval_secs: Option<u64>,
) -> Result<(), String> {
    {
        let state = app.state::<TailState>();
        let running = state.running.lock().await;
        if *running {
            return Err("tail already running; stop it before starting another".to_string());
        }
        drop(running);
        *app.state::<TailState>().running.lock().await = true;
    }
    let interval = std::time::Duration::from_secs(interval_secs.unwrap_or(2).max(1));
    let task_app = app.clone();
    let handle = tokio::spawn(async move {
        let app = task_app;
        let dir = match resolve_outdir(input.cdk_out.as_deref()) {
            Ok(d) => d,
            Err(e) => {
                let _ = app.emit("log-error", e);
                finish_tail(&app).await;
                return;
            }
        };
        let graph = match cdk_graph::ResourceGraph::from_cdk_out(&dir) {
            Ok(g) => g,
            Err(e) => {
                let _ = app.emit("log-error", e.to_string());
                finish_tail(&app).await;
                return;
            }
        };
        let needle = input.construct_path.trim().trim_matches('/').to_string();
        let mut groups: Vec<String> = graph
            .nodes()
            .iter()
            .filter(|n| {
                n.construct_path == needle
                    || n.construct_path.starts_with(&format!("{needle}/"))
            })
            .filter_map(|n| n.log_group.clone())
            .collect();
        groups.sort();
        groups.dedup();
        if let Some(p) = &input.connection.profile {
            if let Err(e) = aws_obs::validate_profile(p) {
                let _ = app.emit("log-error", e.to_string());
                finish_tail(&app).await;
                return;
            }
        }
        let sdk = aws_obs::load_sdk_config(&aws_obs::AwsOptions::new(
            input.connection.profile.clone(),
            input.connection.region.clone(),
        ))
        .await;
        let client = aws_obs::cloudwatch_client(&sdk);
        // Live tail fails hard on missing groups; filter them like search does.
        let missing = aws_obs::find_missing_log_groups(&client, &groups)
            .await
            .unwrap_or_default();
        let present: Vec<String> = groups
            .iter()
            .filter(|g| !missing.contains(g))
            .cloned()
            .collect();
        if present.is_empty() {
            let _ = app.emit(
                "log-error",
                "no log groups exist yet — invoke the function once or deploy the stack",
            );
            finish_tail(&app).await;
            return;
        }

        // Prefer true streaming (StartLiveTail); fall back to polling.
        let filter = input.filter.clone();
        let mut live_started = false;
        loop {
            if !tail_running(&app).await {
                return;
            }
            let emit_app = app.clone();
            let res = aws_obs::live_tail_stream(&client, &present, filter.as_deref(), |events| {
                let batch: Vec<LogLine> = events.iter().map(to_log_line).collect();
                let _ = emit_app.emit("log-batch", &batch);
            })
            .await;
            match res {
                Ok(()) => {
                    live_started = true;
                    // Session cap (3h) or server close: reconnect shortly.
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                Err(e) => {
                    if !live_started {
                        let _ = app.emit(
                            "log-error",
                            format!("live tail unavailable ({e}) — falling back to polling"),
                        );
                    } else {
                        let _ = app.emit("log-error", format!("live tail ended: {e}"));
                    }
                    break;
                }
            }
        }

        // Polling fallback (FilterLogEvents).
        let now = aws_obs::now_ms();
        let (mut start_ms, _) = aws_obs::resolve_time_window(
            input.back.as_deref(),
            input.start.as_deref(),
            input.end.as_deref(),
            now,
        )
        .unwrap_or((Some(now - 3_600_000), Some(now)));
        let limit = input.limit.unwrap_or(100).clamp(1, 1000);
        while tail_running(&app).await {
            let page = aws_obs::filter_log_events(
                &client,
                &aws_obs::LogsQuery {
                    log_groups: present.clone(),
                    start_ms,
                    end_ms: None,
                    filter_pattern: input.filter.clone(),
                    limit: Some(limit),
                },
                None,
            )
            .await;
            match page {
                Ok(p) => {
                    if !p.events.is_empty() {
                        let batch: Vec<LogLine> = p.events.iter().map(to_log_line).collect();
                        start_ms = batch.last().map(|l| l.timestamp_ms.saturating_add(1));
                        let _ = app.emit("log-batch", &batch);
                    }
                }
                Err(e) => {
                    let _ = app.emit("log-error", e.to_string());
                }
            }
            tokio::time::sleep(interval).await;
        }
        finish_tail(&app).await;
    });
    *app.state::<TailState>().handle.lock().await = Some(handle);
    Ok(())
}

async fn tail_running(app: &AppHandle) -> bool {
    *app.state::<TailState>().running.lock().await
}

async fn finish_tail(app: &AppHandle) {
    *app.state::<TailState>().running.lock().await = false;
}

#[tauri::command]
async fn stop_tail(state: State<'_, TailState>) -> Result<(), String> {
    *state.running.lock().await = false;
    let handle = state.handle.lock().await.take();
    if let Some(h) = handle {
        h.abort();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Project inspection + bundled sample (welcome / add-project flow)
// ---------------------------------------------------------------------------

/// What the Add-project dialog needs: is this a CDK project, where is its
/// `cdk.out`, what's inside, and is it stale.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CdkOutInspect {
    cdk_json_found: bool,
    app_command: Option<String>,
    outdir: String,
    stacks: Vec<String>,
    stage_hints: Vec<String>,
    node_count: usize,
    log_group_count: usize,
    out_mtime_ms: Option<i64>,
    src_mtime_ms: Option<i64>,
    stale: bool,
    error: Option<String>,
}

fn mtime_ms(path: &std::path::Path) -> Option<i64> {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}

fn max_mtime_recursive(dir: &std::path::Path, depth: usize) -> Option<i64> {
    if depth > 4 {
        return mtime_ms(dir);
    }
    let mut max = mtime_ms(dir);
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            // Never descend into dependency/build outputs.
            if p.is_dir() {
                if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                    if matches!(
                        name,
                        "node_modules" | "target" | ".git" | "dist" | ".venv" | "__pycache__"
                    ) {
                        continue;
                    }
                }
            }
            let child = if p.is_dir() {
                max_mtime_recursive(&p, depth + 1)
            } else {
                mtime_ms(&p)
            };
            max = max.max(child);
        }
    }
    max
}

fn stage_hints_for(stacks: &[String]) -> Vec<String> {
    const KNOWN: &[&str] = &["dev", "staging", "stage", "prod", "production", "test", "qa"];
    let mut hints: Vec<String> = stacks
        .iter()
        .filter_map(|s| {
            s.rsplit(['-', '_', '/'])
                .next()
                .map(str::to_lowercase)
                .filter(|t| KNOWN.contains(&t.as_str()))
        })
        .collect();
    for fallback in ["dev", "staging", "prod"] {
        if !hints.contains(&fallback.to_string()) {
            hints.push(fallback.to_string());
        }
    }
    hints.sort();
    hints.dedup();
    hints.truncate(6);
    hints
}

#[tauri::command]
async fn inspect_cdk_out(
    project_dir: Option<String>,
    cdk_out: Option<String>,
) -> Result<CdkOutInspect, String> {
    // Resolve the project root (if any) and the outdir.
    let (project_root, app_command, outdir) = match project_dir {
        Some(dir) => {
            let start = std::path::PathBuf::from(&dir);
            match synth::CdkProject::locate(&start) {
                Ok(p) => (
                    Some(p.root.clone()),
                    Some(p.app_command.clone()),
                    cdk_out.map(std::path::PathBuf::from).unwrap_or_else(|| {
                        p.outdir().to_path_buf()
                    }),
                ),
                Err(_) => (
                    None,
                    None,
                    std::path::PathBuf::from(cdk_out.unwrap_or(dir)),
                ),
            }
        }
        None => match cdk_out {
            Some(dir) => (None, None, std::path::PathBuf::from(dir)),
            None => {
                return Err("pick a project folder or a cdk.out directory".to_string());
            }
        },
    };

    let mut out = CdkOutInspect {
        cdk_json_found: project_root.is_some(),
        app_command,
        outdir: outdir.display().to_string(),
        stacks: Vec::new(),
        stage_hints: Vec::new(),
        node_count: 0,
        log_group_count: 0,
        out_mtime_ms: None,
        src_mtime_ms: None,
        stale: false,
        error: None,
    };

    match cdk_graph::ResourceGraph::from_cdk_out(&outdir) {
        Ok(graph) => {
            let mut stacks: Vec<String> = graph
                .nodes()
                .iter()
                .filter_map(|n| n.construct_path.split('/').next().map(str::to_string))
                .collect();
            stacks.sort();
            stacks.dedup();
            out.node_count = graph.len();
            out.log_group_count = graph.log_groups().count();
            out.stage_hints = stage_hints_for(&stacks);
            out.stacks = stacks;
        }
        Err(e) => {
            out.error = Some(e.to_string());
            return Ok(out);
        }
    }

    out.out_mtime_ms = max_mtime_recursive(&outdir, 0);
    if let Some(root) = &project_root {
        let mut src_max = mtime_ms(&root.join("cdk.json"));
        for sub in ["lib", "bin", "src"] {
            let p = root.join(sub);
            if p.is_dir() {
                // Shallow: top-level entries only (they change on every edit
                // that matters for synth input discovery).
                if let Ok(entries) = std::fs::read_dir(&p) {
                    for entry in entries.flatten() {
                        src_max = src_max.max(mtime_ms(&entry.path()));
                    }
                }
            }
        }
        out.src_mtime_ms = src_max;
        out.stale = matches!(
            (src_max, out.out_mtime_ms),
            (Some(s), Some(o)) if s > o
        );
    }
    Ok(out)
}

/// Materialize the bundled fixture graph into the app-data dir (once) and
/// return its nodes + location — powers "Try the bundled sample", no AWS needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SampleGraph {
    nodes: Vec<cdk_graph::ResourceNode>,
    #[serde(default)]
    edges: Vec<cdk_graph::ResourceEdge>,
    cdk_out: String,
}

#[tauri::command]
async fn load_sample_graph(app: AppHandle) -> Result<SampleGraph, String> {
    const TREE: &str = include_str!(
        "../../../crates/cdk-graph/tests/fixtures/sample-cdk-out/tree.json"
    );
    const MANIFEST: &str = include_str!(
        "../../../crates/cdk-graph/tests/fixtures/sample-cdk-out/manifest.json"
    );
    const TEMPLATE: &str = include_str!(
        "../../../crates/cdk-graph/tests/fixtures/sample-cdk-out/SampleStack.template.json"
    );
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("sample-cdk-out");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for (name, contents) in [
        ("tree.json", TREE),
        ("manifest.json", MANIFEST),
        ("SampleStack.template.json", TEMPLATE),
    ] {
        let p = dir.join(name);
        let write = match std::fs::read_to_string(&p) {
            Ok(existing) => existing != contents,
            Err(_) => true,
        };
        if write {
            std::fs::write(&p, contents).map_err(|e| e.to_string())?;
        }
    }
    cdk_graph::ResourceGraph::from_cdk_out(&dir)
        .map(|g| SampleGraph {
            nodes: g.nodes().to_vec(),
            edges: g.edges().to_vec(),
            cdk_out: dir.display().to_string(),
        })
        .map_err(|e| e.to_string())
}

/// Profiles found in ~/.aws/config + ~/.aws/credentials — feeds the
/// stage form dropdown so a typo'd profile is impossible to save.
#[tauri::command]
async fn list_aws_profiles() -> Result<Vec<aws_obs::AwsProfile>, String> {
    Ok(aws_obs::available_profiles())
}

/// Canonical Tauri entry point (kept in the lib so commands can stay
/// private, matching the official template).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            // WS7: forward `cw-trace://debug?...` links to the webview.
            #[cfg(any(target_os = "linux", all(debug_assertions, windows)))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let _ = app.deep_link().register_all();
            }
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    for url in event.urls() {
                        let _ = handle.emit("deep-link-open", url.to_string());
                    }
                });
                if let Ok(Some(urls)) = app.deep_link().get_current() {
                    for url in urls {
                        let _ = app.emit("deep-link-open", url.to_string());
                    }
                }
            }
            Ok(())
        })
        .manage(TailState {
            running: Mutex::new(false),
            handle: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_graph,
            search_logs,
            fetch_trace,
            resolve_request_trace,
            verify_log_groups,
            run_synth,
            test_connection,
            run_insights,
            list_trace_summaries,
            reveal_in_finder,
            open_external,
            open_terminal_with,
            start_tail,
            stop_tail,
            inspect_cdk_out,
            load_sample_graph,
            list_aws_profiles,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run cw-trace");
}
