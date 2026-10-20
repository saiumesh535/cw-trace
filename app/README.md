# cw-trace desktop (M3)

Tauri backend + Svelte frontend over the workspace core crates. Read-only
against AWS through M3.

## Prerequisites

- Stable Rust (edition 2024 for `crates/*`, 2021 for `src-tauri`)
- **pnpm** (this repo uses pnpm, not npm): `corepack enable` or
  `npm i -g pnpm` — then `pnpm --version`
- AWS credentials via the standard chain (profiles/SSO)

## Develop

```sh
cd app
pnpm install
pnpm dev        # Vite on http://localhost:1420
```

In a second terminal (Rust backend):

```sh
cargo tauri dev --manifest-path app/src-tauri/Cargo.toml
```

The backend is intentionally **outside** the Cargo workspace
(`Cargo.toml` members are `crates/*`), so `cargo test --workspace` from the
repo root stays fast with no Tauri/Node dependencies.

## Build

```sh
cd app
pnpm build
cargo tauri build --manifest-path src-tauri/Cargo.toml
```

## Layout

```
app/
  package.json            # pnpm + Vite + Svelte 5
  vite.config.ts          # dev server on :1420 (matches tauri.conf.json)
  index.html
  src/
    main.ts / app.css
    App.svelte            # shell + deep-link (cw-trace://) routing
    lib/api.ts            # invoke('get_graph'|'search_logs'|...) + log-batch listener
    lib/types.ts          # mirrors of the Rust Serialize structs
    lib/trace.ts          # severity + trace/request-id correlation helpers
    lib/deepLink.ts       # cw-trace:// build/parse (WS7)
    components/
      TopBar.svelte       # profile/region/time-range/search (FR-20)
      Debugger.svelte     # tree + logs + trace panes, tail/pause, load-newer, region override
      VirtualLogTable.svelte  # virtualized rows, severity tint, JSON expand, follow scroll
      TracePanel.svelte   # nested waterfall, segment → construct jump
  src-tauri/
    Cargo.toml            # tauri 2 backend, path deps on ../../crates/*
    tauri.conf.json       # pnpm dev/build hooks, dist at ../dist, cw-trace:// deep-link scheme
    src/lib.rs            # graph/logs/trace commands, live StartLiveTail tail + polling fallback
    src/main.rs
```

## Features (M3)

- Construct tree from `cdk.out`; missing log groups flagged (FR-9).
- Log search with cursor pagination ("Load newer", FR-12) and Insights.
- Tail via CloudWatch **StartLiveTail** (`logs:StartLiveTail`) with automatic
  reconnect (3 h session cap) and polling fallback; Pause/Resume keeps the
  buffer, `follow` auto-scrolls unless you scroll up.
- Nested X-Ray waterfall (subsegments, error/fault/throttle, HTTP status);
  click a span to jump to its construct logs.
- Error triage chips: err-only filter, jump first/last error, open the first
  error line's trace.
- Shareable `cw-trace://debug?stage=…&construct=…&trace=…&filter=…&start=…&end=…`
  links ("Link" button); the scheme is registered on install.
- Session region override for multi-region stacks (stage region untouched).

Read-only against AWS — see [`docs/IAM.md`](../docs/IAM.md) for the minimal
policy. Bundles are published to a GitHub Release by
[`.github/workflows/release.yml`](../.github/workflows/release.yml) when you
push a `v*` tag (`git tag v0.1.0 && git push origin v0.1.0`) or run **Release**
manually in Actions. Updater artifacts stay disabled
(`bundle.createUpdaterArtifacts: false`) until a signing key is configured.

