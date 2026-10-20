# UI PRD: cw-trace desktop (M3)

| Field    | Value                                              |
|----------|----------------------------------------------------|
| Status   | Draft for mockups                                  |
| Date     | 2026-10-14                                         |
| Scope    | Desktop app only (`app/`); CLI/core behavior is in `docs/PRD.md` |
| Design ref | SST Console overview (cards + stage rows), adapted to local-first |

## 1. What this app is (the one-paragraph brief)

cw-trace is a **local, read-only debugger for CDK-deployed services**. Point it
at a CDK project, pick a **stage**, and it shows your construct tree next to
that stage's real CloudWatch logs and X-Ray traces — click a trace ID in a log
line to see the waterfall, click a trace segment to jump back to logs. It never
modifies AWS, never phones home, has no accounts or teams. Everything (pins,
recents, settings) lives on the user's machine.

## 2. Goals for the UI

- G1: First-run → debugging in under 2 minutes (welcome explains, then gets out
  of the way).
- G2: Switching between pinned stages (dev/staging/prod of the same infra
  code) is one click — never re-enter profile/region/paths.
- G3: The M2 CLI loop is fully click-driven: tree → tail → trace ID → waterfall
  → back to logs.
- G4: Looks and feels like a modern dark ops console (SST Console is the
  visual reference), but with **no SaaS chrome**: no login, no team, no
  "invite", no workspace switcher.

## 3. Non-goals (UI)

- No login/accounts/team management (SST has these; we explicitly do not).
- No "Add AWS Account" SaaS flow — the app uses the local AWS credential chain
  (profiles + SSO already on the machine).
- No write actions (no deploys, no retention changes, no invokes).
- No multi-window; single window, single active context at a time.

## 4. Core concepts (must be visible in the UI)

| Concept | Definition | Example |
|---------|-----------|---------|
| Project | A CDK project directory (contains `cdk.json`) | `~/work/orders-svc` |
| Stage | A deployment of that infra code: name + `cdk.out` source + profile + region (+ optional stack filter) | `dev` → `cdk.out-dev`, profile `dev-sso`, `us-east-1` |
| Context | The active (project, stage, profile, region) tuple | shown in the top bar |
| Pin | A saved stage shortcut (persisted locally) for one-click switching | sidebar + dashboard strip |

**Stage detection (product decision for mockups):** manual-first. The app
auto-suggests stages from stack-name suffixes (`Orders-Dev`, `Orders-Prod`)
and from multiple `cdk.out*` directories, but the user confirms each stage in
an "Add stage" dialog (name, cdk.out dir, profile, region, stack filter, pin
toggle). Assumption to validate: this matches how the user deploys stages
today (flagged in §11).

## 5. Information architecture

```
Welcome (first run / empty state)
  └── Dashboard (home)
        ├── Project cards → stage rows → opens Debugger with that context
        └── Add project / Add stage dialogs
Debugger (per context: tree | logs | trace)
Settings (profiles, defaults, pins, about)
```

Global **sidebar** on Dashboard + Debugger: Home, Pinned stages, Projects,
Settings (bottom). The Debugger adds a **context top bar** above the panes.

## 6. Screens

### S0 — Welcome (first run and empty state)

Purpose: say what the app does in 10 seconds, then convert to "open a
project". Shown when there are no projects yet; reachable later via an empty
dashboard state.

- Hero: one line — "Stop hunting for log groups." Sub-line: "cw-trace reads
  your `cdk.out`, maps constructs to log groups, and jumps between logs and
  traces."
- 3-step visual (icons, not paragraphs):
  1. Open your CDK project folder.
  2. Pick a stage (dev / prod) — pin it.
  3. Tail logs, click a trace ID, see the waterfall.
- Primary CTA: **Open CDK project** (folder picker). Secondary: **Try the
  bundled sample** (loads the fixture graph, no AWS needed).
- Reassurance row (small, with lock icon): "Read-only. No account. Nothing
  leaves this machine."
- After first project is added, this screen is never shown again (empty
  dashboard takes over for the zero-pinned case).

### S1 — Dashboard (home, SST-inspired)

Purpose: pick up where you left off; see every stage's health at a glance.

- **Sidebar** (see §7).
- **Pinned strip** (top, only if pins exist): pinned stage chips with status
  dot; click switches context and opens the Debugger.
- **Project cards** (SST Overview style, one card per project):
  - Card header: project name + folder path (truncated) + `⋯` menu (add
    stage, remove, reveal in Finder).
  - Stage rows: status dot (green ok / red error / grey unknown-not-checked),
    stage name, pin toggle, profile + region (mono, dim), "last opened 4m"
    age, error badge (`ERROR`, red outline) when the last check failed.
  - Click a stage row → Debugger with that context.
- **Top-right**: `⌘K Jump to` search field (stages, constructs, trace IDs) +
  **Add project** button (no "Invite Team" — that button does not exist here).
- Empty state (no projects): reuse S0 hero compactly + CTA.

### S2 — Add project / Add stage (dialog flow, 2 steps)

- Step 1 "Project": folder picker → app detects `cdk.json`, shows app command
  + default `cdk.out`. Options: **Synthesize now** (runs `cdk synth`) or **Use
  existing `cdk.out`** (with staleness warning if older than sources:
  "`cdk.out` is older than `lib/` — results may be stale").
- Step 2 "Stage": form with Stage name (suggest `dev`/`prod` from stack
  suffixes), `cdk.out` directory (pre-filled, editable — this is how one
  codebase gets many stages), AWS profile (dropdown from `~/.aws/config` +
  "default chain"), Region (dropdown, pre-filled from config/env), optional
  Stack filter, **Pin this stage** toggle (default on for the first stage).
- Save → stage row appears on the dashboard; if pinned, also in sidebar +
  strip.

### S3 — Stage debugger (the core, 3 panes + context bar)

Purpose: the M2 CLI loop, click-driven.

- **Context bar** (below sidebar, above panes): project/stage switcher
  (dropdown of pins), profile/region switchers, time-range picker
  (`15m 1h 3h 24h 7d` + custom), search/filter field, **Tail** toggle,
  `⌘K` hint. Staleness banner slot ("`cdk.out-dev` is 3 days old —
  re-synthesize") and verify-warning slot ("2 groups not found in
  CloudWatch").
- **Left — construct tree**: search filter at top; nodes grouped by stack;
  each row: construct path, resource-type icon/kind label, log-group presence
  dot (filled = mapped, hollow = none), confidence tag on hover (`resolved`
  vs `inferred`). Selecting a node (or a whole stack = multi-group tail)
  loads the center pane.
- **Center — logs**: virtualized table (must stay smooth at 100k+ rows):
  timestamp, stream (dim), message (mono, wrapping off, horizontal scroll);
  trace/request IDs rendered as **clickable pills**; auto-scroll toggle for
  tail; pause on scroll-up; filter applies client-side after fetch; empty
  state ("No events in the last 3h — widen the range").
- **Right — trace detail/waterfall**: shown after clicking a trace pill:
  trace ID header (copy button), duration, segment bars proportional to time
  (name + origin + duration on hover/click); clicking a segment shows
  **"View logs around this segment"** CTA (computes ±5 min window and loads
  the matching log groups) — this is the trace→logs jump.
- **Inline auth**: SSO expiry shows an inline banner in the affected pane
  ("SSO token expired — run `aws sso login --profile X`" + Retry), never a
  modal.

### S4 — Settings

- AWS: default profile/region, SSO note; Tail defaults (interval, batch
  size); Pins list (reorder/remove); Appearance (follow system — dark only
  for v1); About (version, core thesis line, link to docs). No account pages.

## 7. Sidebar spec (global, Dashboard + Debugger)

```
Home (⌘1)
PINNED
  ● dev — orders-svc          (dot = last status; ⌘2, ⌘3… for first 9)
  ● prod — orders-svc
PROJECTS
  orders-svc
  billing-svc
  + Add project
────────
Settings (bottom, ⌘,)
```

- Pinned rows show a status dot + unpin on hover; overflow collapses.
- Active context is highlighted; clicking a pin switches context in place
  (Debugger stays open, panes reload).
- No workspace/org switcher, no user avatar — there is no login.

## 8. Component inventory (what mockups must cover)

Buttons (primary/ghost/danger), project card + stage row, status
dot + `ERROR` badge, pin toggle (pinned/unpinned/hover), context switcher
dropdowns, time-range picker (+custom popover), virtualized log table row
(+ trace pill, + copy-on-click), waterfall bar row, `⌘K` palette
(grouped results: stages / constructs / recent trace IDs), banners
(stale / verify / auth, all inline + dismissible), empty states (no
projects / no events / no trace selected), dialog (2-step add flow),
tooltips (confidence, log-group name).

## 9. Visual direction (SST-inspired, not cloned)

- Dark navy background, slightly lighter cards with 1px borders and 8–12px
  radii (per the reference screenshot).
- Mono font for IDs/paths/regions/ages; sans for labels.
- One accent color for primary actions (SST uses orange; pick ours and use it
  sparingly), red reserved for errors, green/grey dots for stage status.
- Density: dashboard rows ~48px, log table 12–13px mono. Desktop-first,
  min window 1280×800; no mobile story.

## 10. Copy deck (use verbatim in mockups)

- Hero: "Stop hunting for log groups."
- Sub: "Point cw-trace at your CDK project. It maps constructs to log groups
  and jumps between logs and traces."
- "Read-only. No account. Nothing leaves this machine."
- Empty logs: "No events in this range — widen it or check the filter."
- Stale: "`{outdir}` is older than your sources — results may be stale."
- Auth: "SSO token expired — `aws sso login --profile {p}`, then Retry."

## 11. Open questions for mockup review

- O1: How do stages map to `cdk.out` in your flow — separate `cdk.out-dev/`
  dirs, re-synth with context flags, or stack-name suffixes only? (Determines
  how smart "Add stage" auto-detect can be.)
- O2: What identifies a deploy in a stage row — commit SHA, synth time, "last
  opened"? (SST shows commit + age; we have no CI — propose "last opened".)
- O3: Should pins sync across machines (dotfiles) or stay machine-local?
  (Propose local-only for v1.)

## 12. Mockup acceptance ("done")

- 4 screens (Welcome, Dashboard w/ pinned + unpinned states, Add
  project/stage dialog, Debugger w/ logs + trace-open states) + Settings.
- Component sheet (§8) + one empty-state and one error-state per screen.
- Clickable or annotated flows: first-run → dashboard → debugger →
  log→trace→logs jump; pin/unpin; stage switch.
