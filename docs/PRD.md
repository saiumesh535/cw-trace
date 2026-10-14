# PRD: cw-trace

| Field    | Value                                              |
|----------|----------------------------------------------------|
| Status   | Draft (M0)                                         |
| Date     | 2026-10-14                                         |
| Owner    | saiumesh535                                        |
| Repo     | `cw-trace` (sibling of `dir-kill`)                 |

## 1. Problem

Debugging a CDK-deployed service means hopping between disconnected surfaces:

1. Read infra code (or CloudFormation console) to figure out the *actual* log group name — CDK auto-generates names (`/aws/lambda/<stack>-<construct>-<hash>`) that nobody memorizes.
2. Open CloudWatch Logs, find the group, scroll/tail, spot an error.
3. Copy a request/trace ID out of a log line.
4. Open X-Ray, paste the ID, reconstruct what happened across API Gateway → Lambda → DynamoDB.

Each step is a context switch; the construct → log-group → trace mapping lives only in the engineer's head. Existing tools cover slices (terminal tailers, the AWS console, SST Console for SST-only apps, paid SaaS with agents) but nothing reads an arbitrary plain-CDK repo and unifies logs + traces around its construct tree.

## 2. Users

- **Primary:** backend engineers operating plain `aws-cdk` apps (any CDK language) who debug via CloudWatch + X-Ray today.
- **Secondary:** platform teams supporting many CDK services across accounts/regions.

## 3. Goals

- G1: Given a CDK project directory, produce the construct → log-group map with zero manual input.
- G2: Tail/search a construct's logs in one action (no log-group name required).
- G3: Jump bidirectionally between a log line and its X-Ray trace.
- G4: CLI proves the full loop first; desktop GUI (Tauri + Svelte) productizes it.
- G5: Read-only against AWS through M3 — the tool never mutates infrastructure, data, or retention.

## 4. Non-goals (through M4)

- N1: Terraform / CloudFormation-direct / Pulumi / SAM parsing (CDK-only; revisit post-M4).
- N2: Metrics, alarms, dashboards (logs + traces only).
- N3: Write actions (restart service, set retention, invoke Lambda, replay events).
- N4: Mapping constructs back to source file/line (CDK emits no source maps).
- N5: SaaS, accounts, telemetry, or any data leaving the user's machine.

## 5. Locked decisions

| Decision      | Choice                                                        |
|---------------|---------------------------------------------------------------|
| IaC input     | CDK only; parse `cdk.out/`, never CDK source                  |
| `cdk.out`     | Tool runs `cdk synth` itself (reads `app` from `cdk.json`)    |
| UI            | Rust core + Tauri backend + Svelte frontend (M3)              |
| Build order   | CLI-first core (M0–M2), desktop GUI second (M3)               |
| Debug scope   | CloudWatch Logs + X-Ray traces with bidirectional correlation |
| AWS auth      | Standard credential chain: profiles + SSO, `--profile`/`--region` overrides |
| Mutability    | Strictly read-only through M3                                 |
| Region        | Single-region first; multi-region after M2                    |
| Repo          | New repo `cw-trace`, private initially                        |

## 6. Architecture

```
┌─────────────┐   synth    ┌──────────┐  parse   ┌────────────┐
│ CDK project │ ─────────▶ │ cdk.out/ │ ───────▶ │ cdk-graph  │
│ (cdk.json)  │  (synth    │ tree.json│          │ Resource-  │
└─────────────┘   crate)   │ manifest │          │ Graph      │
                            │ *.tmpl   │          └─────┬──────┘
                            └──────────┘                │ construct → log group
┌─────────────┐  tail/search ┌──────────┐  extract ┌────▼───────┐  jump ┌──────────┐
│ CloudWatch  │ ◀──────────▶ │ aws-obs  │ ───────▶ │ correlate  │ ◀───▶ │ X-Ray    │
│ Logs        │              └──────────┘  IDs     └────────────┘       │ traces   │
└─────────────┘                                                        └──────────┘
      ▲                          ▲
      │ commands/events          │ commands/events
┌─────┴────────┐          ┌───────┴────────┐
│ CLI (M0–M2)  │          │ Tauri+Svelte   │
└──────────────┘          │ desktop (M3)   │
                          └────────────────┘
```

### Crates

- **`synth`**: locate `cdk.json` (walk up from cwd), read `app` command + `output` dir, shell out to synthesize, return paths to `tree.json` / `manifest.json`. Staleness check: warn when `cdk.out` predates `lib/`/`bin/` sources.
- **`cdk-graph`**: walk `tree.json` construct nodes (`aws:cdk:cloudformation:type`, `aws:cdk:cloudformation:props`, `constructInfo.fqn`); join with `manifest.json` logical-ID metadata and synthesized templates for resolved values. Emit `ResourceGraph`: Lambda (+`FunctionName`), explicit `LogGroup` (+`LogGroupName`), API GW (access-log destinations), ECS (`awslogs-group`), StepFunctions (logging config). Unresolved/auto-generated names kept as patterns and verified live via `DescribeLogGroups`.
- **`aws-obs`**: `aws-config` credential chain; CloudWatch Logs `FilterLogEvents` (tail/search, default) and `StartQuery` (Insights aggregations, explicit user action); X-Ray `GetTraceSummaries` + `BatchGetTraces`.
- **`correlate`** (no AWS calls): extract X-Ray trace IDs (`1-<8hex>-<24hex>`), Lambda `RequestId:` values, API GW extended request IDs from log text; build jump targets (log line → trace; trace segment → log group + time window).
- **`cli`**: thin verbs over the core: `status` (M0), `graph` (M1), `logs <ConstructPath>` (M2), `trace <id>` (M2).
- **`app/`** (M3): `src-tauri` commands over the core + Svelte SPA: construct-tree panel, virtualized log table, trace waterfall, profile/region + time-range bar, streaming tail via batched `log-batch` events.

## 7. Functional requirements

### Project & synthesis
- FR-1: `locate` finds `cdk.json` walking up from any subdirectory; errors clearly when absent.
- FR-2: Parse `app` (required) and `output` (default `cdk.out`) from `cdk.json`.
- FR-3: Run the `app` command with a timeout; surface synth stderr verbatim on failure; fall back to existing `cdk.out` only with an explicit staleness warning.
- FR-4: Staleness detection via mtime comparison of sources vs `cdk.out`.

### Resource graph
- FR-5: Parse `tree.json` construct paths, CFN types/props, and `constructInfo.fqn`.
- FR-6: Resolve construct path → CFN logical ID via `manifest.json` metadata.
- FR-7: Extract resolved log-group names from templates (explicit `LogGroupName`, ECS log config, API GW access logs).
- FR-8: Derive conventional names (`/aws/lambda/<function-name>`) when no explicit group exists; mark confidence (resolved vs inferred).
- FR-9: Verify every mapped group with `DescribeLogGroups`; flag missing/stale in UI.

### Logs
- FR-10: Tail a construct's logs (follow mode) without requiring the group name.
- FR-11: Time-bounded search with CloudWatch filter syntax + human-friendly ranges (`-b3h`, `-b2d`).
- FR-12: Multi-group tail for a whole stack/construct subtree.
- FR-13: Default to `FilterLogEvents`; Logs Insights only on explicit request (cost/latency).

### Traces & correlation
- FR-14: Fetch X-Ray summaries for a service + time window; fetch full trace by ID.
- FR-15: Extract trace/request IDs from log lines (plain + JSON-embedded).
- FR-16: Log line → trace jump; trace segment → log group + time-window jump.

### CLI UX (M0–M2)
- FR-17: Global `--profile` / `--region` flags; otherwise standard chain.
- FR-18: `status`, `graph`, `logs`, `trace` verbs with machine-readable (`--json`) output.

### Desktop UX (M3)
- FR-19: Left: construct tree (from graph). Center: virtualized log table (100k+ rows). Right: trace detail/waterfall.
- FR-20: Top bar: profile/region switcher, time-range picker, search.
- FR-21: Streaming tail via batched Tauri events (never one event per line).
- FR-22: Clickable IDs in log lines (trace/request) that jump to the trace panel.

### Auth & packaging
- FR-23: Reuse `~/.aws/config` profiles and SSO tokens; surface re-auth errors in UI.
- FR-24: Per-OS bundles (DMG/MSI/AppImage) + auto-updater via Tauri (M4).

## 8. Non-functional requirements

- NFR-1: `cargo test --workspace` green with no AWS credentials (fixtures + replay only).
- NFR-2: Log table stays interactive at 100k+ rows (virtualization on desktop; paging/streaming on CLI).
- NFR-3: No AWS mutation through M3 (defense in depth: read-only IAM guidance in docs).
- NFR-4: First tail visible in < 3s on warm credentials (synth excluded; synth runs once and caches).
- NFR-5: macOS (primary), Linux, Windows builds in CI from M0.

## 9. Milestones & acceptance criteria

- **M0 — scaffold + synth locate.** Workspace builds; `CdkProject::locate` tested against fixtures; CLI `status` works. Accept: `cargo build/test --workspace` green, no AWS, no Node.
- **M1 — resource graph.** `tree.json` + manifest + template parsing over `crates/cdk-graph/tests/fixtures/sample-cdk-out`; CLI `graph` prints construct → log-group map. Accept: unit + fixture tests assert the mapping.
- **M2 — logs + traces + correlation (CLI).** `aws-obs` + `correlate` wired to `logs <ConstructPath>` and `trace <id>`; end-to-end against a real Lambda+API GW stack. Accept: construct → tail → trace-ID → trace → back to logs, all from the CLI.
- **M3 — desktop app.** Tauri + Svelte over the same core: tree panel, virtual log table, waterfall, profile/region/time controls, batched streaming. Accept: the M2 flow click-driven, no terminal.
- **M4 — ship.** Bundles per OS, updater, staleness UX, read-only IAM docs, public repo + release.

## 10. Testing strategy

- Unit tests with committed fixtures (`cdk.out` samples, log lines, trace JSON) — no AWS.
- AWS paths tested against recorded JSON fixtures (replay harness), never live accounts in CI.
- Manual end-to-end on a real stack for M2/M3 acceptance only.
- Desktop: Tauri + frontend component tests for table/waterfall in M3.

## 11. Risks

| Risk | Mitigation |
|------|------------|
| `tree.json` is an internal CDK format (v`tree-0.1`), not a public API | Pin parser to observed schema; fixture tests per CDK major; degrade gracefully on unknown nodes |
| Auto-generated names unresolvable statically | Heuristic + pattern fallback + live `DescribeLogGroups` verification; confidence labels |
| `cdk synth` requires Node + app deps on user machine | Clear prerequisite error; reuse existing `cdk.out` with staleness warning |
| Log volume cost/latency | Tight default time windows; Insights opt-in; batched streaming |
| SSO token expiry mid-session | Surface re-auth inline; retry once after refresh |
| Desktop distribution burden | Tauri bundler + updater (key reason for Tauri over gpui-kit) |
| Linux WebView variance | Pin minimum WebKitGTK; test Ubuntu LTS + Fedora |

## 12. Open questions

- O1: Public flip timing (currently private; propose public at M2 when CLI proves out)?
- O2: Multi-region support — M2 or later?
- O3: Any post-M4 write actions worth scoping early (e.g. set retention)?
