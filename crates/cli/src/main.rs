//! `cw-trace`: CDK-aware CloudWatch log + X-Ray trace debugger.
//!
//! Thin CLI over the workspace core crates: `status` / `graph` (M1) and
//! `logs` / `trace` (M2).

use anyhow::Context;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "cw-trace", version, about = "CDK-aware CloudWatch log + X-Ray trace debugger")]
struct Cli {
    /// AWS profile (defaults to the standard credential chain).
    #[arg(long, global = true)]
    profile: Option<String>,

    /// AWS region (defaults to the standard credential chain).
    #[arg(long, global = true)]
    region: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show tool status.
    Status,
    /// Print the construct → log-group map from `cdk.out/` (M1).
    Graph {
        /// Path to `cdk.out/` directly (overrides project discovery).
        #[arg(long)]
        cdk_out: Option<PathBuf>,
        /// Project directory to start `cdk.json` discovery from (defaults to cwd).
        #[arg(long)]
        project_dir: Option<PathBuf>,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Tail/search a construct's logs without naming the log group (M2).
    Logs {
        /// Construct path or subtree prefix, e.g. `SampleStack/CreateOrder`.
        construct_path: String,
        /// Path to `cdk.out/` directly (overrides project discovery).
        #[arg(long)]
        cdk_out: Option<PathBuf>,
        /// Project directory to start `cdk.json` discovery from (defaults to cwd).
        #[arg(long)]
        project_dir: Option<PathBuf>,
        /// CloudWatch filter pattern (filter syntax).
        #[arg(long)]
        filter: Option<String>,
        /// Look back this far (e.g. 30m, 3h, 2d). Defaults to 1h.
        #[arg(short = 'b', long)]
        back: Option<String>,
        /// Absolute start (RFC 3339 or epoch secs/ms).
        #[arg(long)]
        start: Option<String>,
        /// Absolute end (RFC 3339 or epoch secs/ms, defaults to now).
        #[arg(long)]
        end: Option<String>,
        /// Max events per request (default 100).
        #[arg(long, default_value_t = 100)]
        limit: i32,
        /// Keep polling for new events.
        #[arg(short = 'f', long)]
        follow: bool,
        /// Poll interval in seconds for `--follow` (default 2).
        #[arg(long, default_value_t = 2)]
        interval: u64,
        /// Run a Logs Insights query instead of FilterLogEvents (explicit opt-in).
        #[arg(long)]
        insights: Option<String>,
        /// Warn when mapped log groups do not exist in CloudWatch.
        #[arg(long)]
        verify: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Fetch an X-Ray trace or list summaries (M2).
    Trace {
        /// Full trace ID (`1-<8hex>-<24hex>`). Omit with `--service` to list summaries.
        id: Option<String>,
        /// X-Ray service name for summary mode (sets `service("<name>")` filter).
        #[arg(long)]
        service: Option<String>,
        /// Raw X-Ray filter expression for summary mode.
        #[arg(long)]
        filter: Option<String>,
        /// Look back this far for summary mode (default 1h).
        #[arg(short = 'b', long)]
        back: Option<String>,
        /// Absolute start (RFC 3339 or epoch secs/ms).
        #[arg(long)]
        start: Option<String>,
        /// Absolute end (RFC 3339 or epoch secs/ms).
        #[arg(long)]
        end: Option<String>,
        /// Max summaries to fetch (default 20).
        #[arg(long, default_value_t = 20)]
        limit: i32,
        /// Path to `cdk.out/` for trace → logs suggestions.
        #[arg(long)]
        cdk_out: Option<PathBuf>,
        /// Project directory for `cdk.json` discovery.
        #[arg(long)]
        project_dir: Option<PathBuf>,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Status => {
            println!("cw-trace {}", env!("CARGO_PKG_VERSION"));
            println!(
                "profile: {}",
                cli.profile.as_deref().unwrap_or("(default chain)")
            );
            println!(
                "region: {}",
                cli.region.as_deref().unwrap_or("(default chain)")
            );
        }
        Command::Graph {
            cdk_out,
            project_dir,
            json,
        } => {
            run_graph(cdk_out, project_dir, json)?;
        }
        Command::Logs {
            construct_path,
            cdk_out,
            project_dir,
            filter,
            back,
            start,
            end,
            limit,
            follow,
            interval,
            insights,
            verify,
            json,
        } => {
            run_logs(
                &cli.profile,
                &cli.region,
                LogsArgs {
                    construct_path,
                    cdk_out,
                    project_dir,
                    filter,
                    back,
                    start,
                    end,
                    limit,
                    follow,
                    interval,
                    insights,
                    verify,
                    json,
                },
            )
            .await?;
        }
        Command::Trace {
            id,
            service,
            filter,
            back,
            start,
            end,
            limit,
            cdk_out,
            project_dir,
            json,
        } => {
            run_trace(
                &cli.profile,
                &cli.region,
                TraceArgs {
                    id,
                    service,
                    filter,
                    back,
                    start,
                    end,
                    limit,
                    cdk_out,
                    project_dir,
                    json,
                },
            )
            .await?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// graph (M1)
// ---------------------------------------------------------------------------

fn run_graph(
    cdk_out: Option<PathBuf>,
    project_dir: Option<PathBuf>,
    json: bool,
) -> anyhow::Result<()> {
    let outdir = resolve_cdk_out(cdk_out, project_dir)?;
    let graph = cdk_graph::ResourceGraph::from_cdk_out(&outdir)
        .with_context(|| format!("failed to build resource graph from {}", outdir.display()))?;
    if json {
        println!("{}", serde_json::to_string_pretty(graph.nodes())?);
    } else {
        println!("# cdk.out: {}", outdir.display());
        println!("# resources: {}", graph.len());
        for node in graph.nodes() {
            let (group, confidence) = match (&node.log_group, &node.confidence) {
                (Some(g), Some(c)) => {
                    let label = match c {
                        cdk_graph::Confidence::Resolved => "resolved",
                        cdk_graph::Confidence::Inferred => "inferred",
                    };
                    (g.clone(), format!(" ({label})"))
                }
                (Some(g), None) => (g.clone(), String::new()),
                _ => ("(no log group)".to_string(), String::new()),
            };
            println!(
                "{}\n  type: {}\n  logs: {}{}",
                node.construct_path, node.cfn_type, group, confidence
            );
        }
    }
    Ok(())
}

fn resolve_cdk_out(
    cdk_out: Option<PathBuf>,
    project_dir: Option<PathBuf>,
) -> anyhow::Result<PathBuf> {
    if let Some(dir) = cdk_out {
        return Ok(dir);
    }
    let start = match project_dir {
        Some(dir) => dir,
        None => std::env::current_dir().context("failed to read current directory")?,
    };
    let project = synth::CdkProject::locate(&start).with_context(|| {
        format!(
            "no cdk.json found at or above {}; pass --cdk-out <dir> to point at cdk.out/ directly",
            start.display()
        )
    })?;
    Ok(project.outdir().to_path_buf())
}

/// Best-effort graph load for trace → logs suggestions (None when absent).
fn try_load_graph(cdk_out: Option<PathBuf>, project_dir: Option<PathBuf>) -> Option<cdk_graph::ResourceGraph> {
    let outdir = match cdk_out {
        Some(dir) => dir,
        None => {
            let start = project_dir
                .or_else(|| std::env::current_dir().ok())?;
            synth::CdkProject::locate(&start).ok()?.outdir().to_path_buf()
        }
    };
    cdk_graph::ResourceGraph::from_cdk_out(&outdir).ok()
}

// ---------------------------------------------------------------------------
// logs (M2)
// ---------------------------------------------------------------------------

struct LogsArgs {
    construct_path: String,
    cdk_out: Option<PathBuf>,
    project_dir: Option<PathBuf>,
    filter: Option<String>,
    back: Option<String>,
    start: Option<String>,
    end: Option<String>,
    limit: i32,
    follow: bool,
    interval: u64,
    insights: Option<String>,
    verify: bool,
    json: bool,
}

/// Exact match or subtree prefix (`SampleStack` matches everything under it).
fn resolve_log_groups(
    graph: &cdk_graph::ResourceGraph,
    prefix: &str,
) -> anyhow::Result<Vec<String>> {
    let needle = prefix.trim().trim_matches('/').to_string();
    let mut groups = Vec::new();
    for node in graph.nodes() {
        let path = &node.construct_path;
        let matches = path == &needle || path.starts_with(&format!("{needle}/"));
        if matches && let Some(group) = &node.log_group
        {
            groups.push(group.clone());
        }
    }
    groups.sort();
    groups.dedup();
    if groups.is_empty() {
        let available: Vec<&str> = graph
            .nodes()
            .iter()
            .take(20)
            .map(|n| n.construct_path.as_str())
            .collect();
        anyhow::bail!(
            "no log groups for construct {prefix:?} (exact or subtree match). Available constructs include: {}",
            available.join(", ")
        );
    }
    Ok(groups)
}

fn format_ts(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| ms.to_string())
}

#[derive(Debug, serde::Serialize)]
struct LogJsonLine {
    timestamp_ms: i64,
    timestamp: String,
    log_group: String,
    log_stream: String,
    message: String,
    trace_ids: Vec<String>,
    lambda_request_ids: Vec<String>,
    apigw_request_ids: Vec<String>,
}

fn to_json_line(e: &aws_obs::LogEvent) -> LogJsonLine {
    let jump = correlate::extract_jumps(&e.message);
    LogJsonLine {
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

fn print_log_human(e: &aws_obs::LogEvent) {
    let jump = correlate::extract_jumps(&e.message);
    let mut suffix = String::new();
    if !jump.trace_ids.is_empty() {
        suffix.push_str(&format!(" trace:{}", jump.trace_ids.join(",")));
    }
    if !jump.lambda_request_ids.is_empty() {
        suffix.push_str(&format!(" request:{}", jump.lambda_request_ids.join(",")));
    }
    if !jump.apigw_request_ids.is_empty() {
        suffix.push_str(&format!(" apigw:{}", jump.apigw_request_ids.join(",")));
    }
    // Single-line the message for tail readability.
    let one_line = e.message.replace('\n', "\\n");
    println!(
        "[{} {}/{}] {}{}",
        format_ts(e.timestamp_ms),
        e.log_group,
        e.log_stream,
        one_line,
        suffix
    );
}

async fn run_logs(
    profile: &Option<String>,
    region: &Option<String>,
    args: LogsArgs,
) -> anyhow::Result<()> {
    let outdir = resolve_cdk_out(args.cdk_out, args.project_dir)?;
    let graph = cdk_graph::ResourceGraph::from_cdk_out(&outdir)
        .with_context(|| format!("failed to build resource graph from {}", outdir.display()))?;
    let groups = resolve_log_groups(&graph, &args.construct_path)?;
    let now = aws_obs::now_ms();
    let (mut start_ms, end_ms) = aws_obs::resolve_time_window(
        args.back.as_deref(),
        args.start.as_deref(),
        args.end.as_deref(),
        now,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    let sdk = aws_obs::load_sdk_config(&aws_obs::AwsOptions::new(
        profile.clone(),
        region.clone(),
    ))
    .await;
    let logs = aws_obs::cloudwatch_client(&sdk);

    if args.verify {
        match aws_obs::find_missing_log_groups(&logs, &groups).await {
            Ok(missing) if !missing.is_empty() => {
                eprintln!(
                    "warning: {} log group(s) not found in CloudWatch: {}",
                    missing.len(),
                    missing.join(", ")
                );
            }
            Err(e) => eprintln!("warning: verification failed: {e}"),
            _ => {}
        }
    }

    // Insights path (explicit opt-in only).
    if let Some(query_string) = args.insights {
        let (s, e) = match (start_ms, end_ms) {
            (Some(s), Some(e)) => (s, e),
            _ => anyhow::bail!("Insights requires a bounded time window"),
        };
        let rows = aws_obs::run_insights_query(&logs, &groups, &query_string, s, e, Some(args.limit))
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        if args.json {
            println!("{}", serde_json::to_string_pretty(&rows)?);
        } else {
            println!("# insights: {} row(s) across {} group(s)", rows.len(), groups.len());
            for row in &rows {
                let mut fields: Vec<&String> = row.keys().collect();
                fields.sort();
                let line = fields
                    .iter()
                    .map(|k| format!("{k}={}", row.get(*k).unwrap_or(&String::new())))
                    .collect::<Vec<_>>()
                    .join(" ");
                println!("{line}");
            }
        }
        return Ok(());
    }

    let limit = args.limit.clamp(1, 10_000);
    if !args.json {
        println!(
            "# logs: {} group(s) {} → {}",
            groups.len(),
            start_ms.map(format_ts).unwrap_or_else(|| "(beginning)".to_string()),
            end_ms.map(format_ts).unwrap_or_else(|| "(now)".to_string()),
        );
        for g in &groups {
            println!("#   group: {g}");
        }
    }

    let mut next_token: Option<String> = None;
    let mut last_ts = start_ms;
    // JSON mode buffers one array; human mode streams lines.
    let mut json_out: Vec<LogJsonLine> = Vec::new();
    loop {
        let page = aws_obs::filter_log_events(
            &logs,
            &aws_obs::LogsQuery {
                log_groups: groups.clone(),
                start_ms,
                end_ms: if args.follow { None } else { end_ms },
                filter_pattern: args.filter.clone(),
                limit: Some(limit),
                next_token: next_token.clone(),
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
        for e in &page.events {
            if args.json {
                json_out.push(to_json_line(e));
            } else {
                print_log_human(e);
            }
            last_ts = Some(last_ts.map_or(e.timestamp_ms, |t| t.max(e.timestamp_ms)));
        }
        next_token = page.next_token.clone();
        let done_paging = next_token.is_none();
        if !args.follow {
            // Single search: follow pagination tokens until exhausted.
            if done_paging {
                break;
            }
            continue;
        }
        // Follow mode: advance the window past what we printed, then poll.
        start_ms = last_ts.map(|t| t.saturating_add(1));
        next_token = None;
        if args.json {
            // In follow+JSON mode we flush each batch as its own array so
            // output stays streaming-friendly.
            if !json_out.is_empty() {
                println!("{}", serde_json::to_string(&json_out)?);
                json_out.clear();
            }
        }
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(std::time::Duration::from_secs(args.interval.max(1))) => {}
        }
        // When following without new events, keep polling with the same window.
        let _ = done_paging;
    }
    if args.json && !json_out.is_empty() {
        println!("{}", serde_json::to_string_pretty(&json_out)?);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// trace (M2)
// ---------------------------------------------------------------------------

struct TraceArgs {
    id: Option<String>,
    service: Option<String>,
    filter: Option<String>,
    back: Option<String>,
    start: Option<String>,
    end: Option<String>,
    limit: i32,
    cdk_out: Option<PathBuf>,
    project_dir: Option<PathBuf>,
    json: bool,
}

async fn run_trace(
    profile: &Option<String>,
    region: &Option<String>,
    args: TraceArgs,
) -> anyhow::Result<()> {
    let sdk = aws_obs::load_sdk_config(&aws_obs::AwsOptions::new(
        profile.clone(),
        region.clone(),
    ))
    .await;
    let xray = aws_obs::xray_client(&sdk);

    // Single-trace mode.
    if let Some(id) = args.id {
        if !correlate::is_trace_id(&id) {
            anyhow::bail!("invalid X-Ray trace ID {id:?} (expected 1-<8hex>-<24hex>)");
        }
        let trace = aws_obs::get_trace(&xray, &id)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let (win_start, win_end) = trace_window(&trace);
        let (suggest_start, suggest_end) =
            correlate::suggest_log_window(win_start, win_end, 300);
        let graph = try_load_graph(args.cdk_out, args.project_dir);
        let groups: Vec<String> = graph
            .as_ref()
            .map(|g| {
                let mut v: Vec<String> = g.log_groups().filter_map(|n| n.log_group.clone()).collect();
                v.sort();
                v.dedup();
                v
            })
            .unwrap_or_default();

        if args.json {
            let out = serde_json::json!({
                "trace": trace,
                "suggested_log_window": {
                    "start_ms": suggest_start,
                    "end_ms": suggest_end,
                    "start": format_ts(suggest_start),
                    "end": format_ts(suggest_end),
                },
                "log_groups": groups,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
        } else {
            println!("trace: {}", trace.id);
            if let Some(d) = trace.duration_secs {
                println!("duration: {d:.3}s");
            }
            println!(
                "segments: {} ({} → {})",
                trace.segments.len(),
                format_ts(win_start),
                format_ts(win_end)
            );
            for seg in &trace.segments {
                let dur = (seg.end_ms - seg.start_ms).max(0) as f64 / 1000.0;
                println!(
                    "  {} {} origin:{} {dur:.3}s",
                    seg.name,
                    format_ts(seg.start_ms),
                    seg.origin.as_deref().unwrap_or("-"),
                );
            }
            println!(
                "suggested logs: --start {} --end {}",
                format_ts(suggest_start),
                format_ts(suggest_end)
            );
            if groups.is_empty() {
                println!("hint: pass --cdk-out <dir> to map segments to log groups");
            } else {
                println!("log groups ({}):", groups.len());
                for g in groups.iter().take(10) {
                    println!("  {g}");
                }
                println!(
                    "hint: cw-trace logs <ConstructPath> --start {} --end {}",
                    format_ts(suggest_start),
                    format_ts(suggest_end)
                );
            }
        }
        return Ok(());
    }

    // Summary mode: `trace --service <name>` or `trace --filter <expr>`.
    let filter_expression = match (args.filter, args.service) {
        (Some(expr), _) => expr,
        (None, Some(service)) => format!("service(\"{service}\")"),
        (None, None) => anyhow::bail!("pass a trace ID or --service <name> / --filter <expr>"),
    };
    let now = aws_obs::now_ms();
    let (start_ms, end_ms) = aws_obs::resolve_time_window(
        args.back.as_deref(),
        args.start.as_deref(),
        args.end.as_deref(),
        now,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    let (s, e) = match (start_ms, end_ms) {
        (Some(s), Some(e)) => (s, e),
        _ => anyhow::bail!("summary mode requires a bounded time window"),
    };
    let (summaries, _next) = aws_obs::get_trace_summaries(
        &xray,
        &aws_obs::SummaryQuery {
            start_ms: s,
            end_ms: e,
            filter_expression: Some(filter_expression.clone()),
            limit: Some(args.limit.clamp(1, 100)),
            next_token: None,
        },
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&summaries)?);
    } else {
        println!(
            "# traces: {} ({} → {}, filter: {})",
            summaries.len(),
            format_ts(s),
            format_ts(e),
            filter_expression
        );
        for t in &summaries {
            let flags = match (t.has_fault, t.has_error) {
                (true, _) => " FAULT",
                (_, true) => " ERROR",
                _ => "",
            };
            let dur = t.duration_secs.map(|d| format!(" {d:.3}s")).unwrap_or_default();
            println!("{}{}{}", t.id, dur, flags);
        }
        if summaries.is_empty() {
            println!("hint: widen --back or adjust --filter");
        } else {
            println!("hint: cw-trace trace <id> for full detail");
        }
    }
    Ok(())
}

fn trace_window(trace: &aws_obs::XRayTrace) -> (i64, i64) {
    let mut start = i64::MAX;
    let mut end = i64::MIN;
    for seg in &trace.segments {
        start = start.min(seg.start_ms);
        end = end.max(seg.end_ms);
    }
    if start == i64::MAX || end == i64::MIN {
        let now = aws_obs::now_ms();
        return (now - 60_000, now);
    }
    (start, end)
}
