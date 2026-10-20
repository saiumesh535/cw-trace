<script lang="ts">
  import { api, onLogBatch, onLogError } from "../lib/api";
  import { friendlyAwsError, type FriendlyAwsError } from "../lib/awsErrors";
  import { cloudWatchLogGroupUrl, xrayTracesUrl } from "../lib/consoleLinks";
  import { buildDeepLink } from "../lib/deepLink";
  import { loadStagePrefs, rememberFilter, saveStagePrefs } from "../lib/stagePrefs";
  import { store } from "../lib/store.svelte";
  import type {
    LogLine,
    LogsCursor,
    NodeHeat,
    ResourceEdge,
    ResourceNode,
    XRayTrace,
  } from "../lib/types";
  import { constructsAlongTrace, severityOf, traceIdsForLine } from "../lib/trace";
  import ConstructMap from "./ConstructMap.svelte";
  import VirtualLogTable from "./VirtualLogTable.svelte";
  import TracePanel from "./TracePanel.svelte";
  import { onMount } from "svelte";

  let { onNodes = () => {} }: { onNodes?: (nodes: ResourceNode[]) => void } = $props();

  const PRESETS = ["15m", "1h", "3h", "24h", "7d"];
  const REGION_CHOICES = [
    "us-east-1",
    "us-east-2",
    "us-west-1",
    "us-west-2",
    "eu-west-1",
    "eu-west-2",
    "eu-central-1",
    "eu-north-1",
    "ap-south-1",
    "ap-southeast-1",
    "ap-southeast-2",
    "ap-northeast-1",
    "ap-northeast-2",
    "ca-central-1",
    "sa-east-1",
  ];
  const DEFAULT_INSIGHTS =
    "fields @timestamp, @message | sort @timestamp desc | limit 50";

  type Banner =
    | { kind: "stale"; title: string; hint?: string }
    | { kind: "missing"; title: string; hint?: string; group?: string }
    | ({ kind: "auth" | "error" } & FriendlyAwsError);

  let nodes = $state<ResourceNode[]>([]);
  let edges = $state<ResourceEdge[]>([]);
  let selected: string | null = $state(null);
  let treeFilter = $state("");
  let treeMode = $state<"list" | "map">("list");
  let heat = $state<Record<string, NodeHeat>>({});
  let focusSpanName = $state<string | null>(null);
  let lines = $state<LogLine[]>([]);
  /** Log groups missing for the *current* search selection. */
  let missing = $state<string[]>([]);
  /** All graph log groups missing in CloudWatch (FR-9 tree dots). */
  let verifiedMissing = $state<string[]>([]);
  let selectedLine = $state<LogLine | null>(null);
  let errorsOnly = $state(false);
  let gotoMs = $state<number | null>(null);
  let filter = $state("");
  let regexOn = $state(false);
  let back = $state("3h");
  let useCustom = $state(false);
  let customStart = $state("");
  let customEnd = $state("");
  let savedFilters = $state<string[]>([]);
  let tailing = $state(false);
  let paused = $state(false);
  let autoScroll = $state(true);
  /** WS9: session-only region override (null = stage region). */
  let regionOverride = $state<string | null>(null);
  let loading = $state(false);
  let nextCursor = $state<LogsCursor | null>(null);
  let loadingMore = $state(false);
  let banner = $state<Banner | null>(null);
  let bannerDetails = $state(false);
  let cmdCopied = $state(false);
  let toast = $state("");
  let trace = $state<XRayTrace | null>(null);
  let traceErr = $state("");
  let traceHint = $state("");
  let traceCmd = $state<string | undefined>(undefined);
  let traceLoading = $state(false);
  let staleChecked = $state(false);
  let verified = $state(false);
  let synthesizing = $state(false);
  let insightsOpen = $state(false);
  let insightsQuery = $state(DEFAULT_INSIGHTS);
  let insightsRows = $state<Record<string, string>[]>([]);
  let insightsBusy = $state(false);
  let insightsErr = $state("");
  let unlistens: (() => void)[] = [];

  function flash(msg: string) {
    toast = msg;
    setTimeout(() => (toast = ""), 2200);
  }

  function persistPrefs() {
    const s = store.activeStage();
    if (!s) return;
    saveStagePrefs(s.id, {
      filter,
      regexOn,
      back,
      useCustom,
      customStart,
      customEnd,
      savedFilters,
    });
  }

  function restorePrefs(stageId: string) {
    const p = loadStagePrefs(stageId);
    filter = p.filter ?? "";
    regexOn = p.regexOn ?? false;
    back = p.back ?? "3h";
    useCustom = p.useCustom ?? false;
    customStart = p.customStart ?? "";
    customEnd = p.customEnd ?? "";
    savedFilters = p.savedFilters ?? [];
  }

  function windowArgs(): { back?: string; startMs?: number; endMs?: number } {
    if (useCustom && customStart && customEnd) {
      const s = Date.parse(customStart);
      const e = Date.parse(customEnd);
      if (Number.isFinite(s) && Number.isFinite(e) && e > s) {
        return { startMs: s, endMs: e };
      }
    }
    return { back };
  }

  function regionOf(s: { region?: string } | null | undefined): string | undefined {
    return regionOverride ?? s?.region;
  }

  function awsCtx() {
    const s = store.activeStage();
    return { profile: s?.profile, region: regionOf(s) };
  }

  function setAwsBanner(err: unknown) {
    const f = friendlyAwsError(err, awsCtx());
    banner = { ...f, kind: f.kind };
    bannerDetails = false;
    if (f.kind !== "auth") {
      const s = store.activeStage();
      if (s) store.markStageError(s.id, f.title);
    }
  }

  async function copyCommand(cmd: string) {
    try {
      await navigator.clipboard.writeText(cmd);
      cmdCopied = true;
      setTimeout(() => (cmdCopied = false), 2000);
    } catch {
      // ignore
    }
  }

  async function openTerminalLogin(cmd: string) {
    try {
      await navigator.clipboard.writeText(cmd);
      await api.openTerminalWith(cmd);
      flash("Copied + opened Terminal");
    } catch (e) {
      flash(String(e));
    }
  }

  async function copyText(text: string, label = "Copied") {
    try {
      await navigator.clipboard.writeText(text);
      flash(label);
    } catch {
      flash("Copy failed");
    }
  }

  function contextSnippet(): string {
    const s = store.activeStage();
    if (!s) return "";
    const win = windowArgs();
    const range = win.startMs
      ? `${new Date(win.startMs).toISOString()} → ${new Date(win.endMs!).toISOString()}`
      : `last ${win.back ?? back}`;
    return [
      `cw-trace context`,
      `project: ${store.projectName(s.projectId)}`,
      `stage: ${s.name}`,
      `profile: ${s.profile ?? "default"}`,
      `region: ${s.region ?? "—"}`,
      `construct: ${selected ?? "—"}`,
      `window: ${range}`,
      selectedLine ? `line: ${selectedLine.message.slice(0, 200)}` : "",
      trace ? `trace: ${trace.id}` : "",
    ]
      .filter(Boolean)
      .join("\n");
  }

  /** WS7: copy a `cw-trace://` link that reopens this exact view. */
  function copyDeepLink() {
    const s = store.activeStage();
    if (!s) return;
    const win = windowArgs();
    const link = buildDeepLink({
      stage: s.name,
      construct: selected ?? undefined,
      trace: trace?.id,
      filter: filter || undefined,
      startMs: win.startMs ?? undefined,
      endMs: win.endMs ?? undefined,
    });
    copyText(link, "Link copied");
  }

  async function revealProject() {
    const s = store.activeStage();
    if (!s) return;
    const proj = store.projects.find((p) => p.id === s.projectId);
    const path = proj?.path ?? s.cdkOut;
    try {
      await api.revealInFinder(path);
    } catch (e) {
      flash(String(e));
    }
  }

  async function openCwConsole() {
    const s = store.activeStage();
    const lg = selectedNode?.log_group;
    const region = regionOf(s);
    if (!region || !lg) {
      flash("Pick a construct with a log group");
      return;
    }
    try {
      await api.openExternal(cloudWatchLogGroupUrl(region, lg));
    } catch (e) {
      flash(String(e));
    }
  }

  async function openXrayConsole() {
    const s = store.activeStage();
    const region = regionOf(s);
    if (!region) return;
    try {
      await api.openExternal(xrayTracesUrl(region));
    } catch (e) {
      flash(String(e));
    }
  }

  async function runInsights() {
    if (!stage || !selected) return;
    insightsBusy = true;
    insightsErr = "";
    insightsRows = [];
    try {
      const win = windowArgs();
      insightsRows = await api.runInsights({
        connection: { profile: stage.profile, region: regionOf(stage) },
        cdkOut: stage.cdkOut,
        constructPath: selected,
        query: insightsQuery,
        back: win.startMs ? undefined : win.back,
        start: win.startMs ? String(win.startMs) : undefined,
        end: win.endMs ? String(win.endMs) : undefined,
      });
    } catch (e) {
      insightsErr = String(e);
    } finally {
      insightsBusy = false;
    }
  }

  const TREE_MIN = 180;
  const TREE_MAX = 520;
  const TRACE_MIN = 220;
  const TRACE_MAX = 640;
  let treeW = $state(readPaneWidth("tree", 280));
  let traceW = $state(readPaneWidth("trace", 360));
  let dragging = $state<"tree" | "trace" | null>(null);

  function readPaneWidth(key: string, fallback: number): number {
    try {
      const n = Number(localStorage.getItem(`cwtrace.pane.${key}`));
      return Number.isFinite(n) && n > 0 ? n : fallback;
    } catch {
      return fallback;
    }
  }

  function savePaneWidth(key: string, w: number) {
    try {
      localStorage.setItem(`cwtrace.pane.${key}`, String(Math.round(w)));
    } catch {
      // ignore
    }
  }

  function clamp(n: number, min: number, max: number) {
    return Math.min(max, Math.max(min, n));
  }

  function startResize(which: "tree" | "trace", e: PointerEvent) {
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    const startX = e.clientX;
    const startTree = treeW;
    const startTrace = traceW;
    dragging = which;
    el.setPointerCapture(e.pointerId);

    const onMove = (ev: PointerEvent) => {
      const dx = ev.clientX - startX;
      if (which === "tree") {
        treeW = clamp(startTree + dx, TREE_MIN, TREE_MAX);
      } else {
        // Handle is on the left edge of the trace pane: drag right → narrower.
        traceW = clamp(startTrace - dx, TRACE_MIN, TRACE_MAX);
      }
    };
    const onUp = (ev: PointerEvent) => {
      el.releasePointerCapture(ev.pointerId);
      el.removeEventListener("pointermove", onMove);
      el.removeEventListener("pointerup", onUp);
      el.removeEventListener("pointercancel", onUp);
      dragging = null;
      savePaneWidth("tree", treeW);
      savePaneWidth("trace", traceW);
    };
    el.addEventListener("pointermove", onMove);
    el.addEventListener("pointerup", onUp);
    el.addEventListener("pointercancel", onUp);
  }

  const stage = $derived(store.activeStage());
  const selectedNode = $derived(nodes.find((n) => n.construct_path === selected) ?? null);
  // Does the current selection map to any log group at all? IAM roles and
  // similar constructs never emit logs — show an inline panel instead of
  // calling the backend (which would just error).
  const selectionHasLogs = $derived.by(() => {
    if (!selected) return false;
    return nodes.some(
      (n) =>
        (n.construct_path === selected || n.construct_path.startsWith(selected + "/")) &&
        !!n.log_group,
    );
  });
  // Client-side refinement over fetched lines (Regex/Case toggles).
  const filtered = $derived.by((): { rows: LogLine[]; bad: boolean } => {
    if (!filter) return { rows: lines, bad: false };
    if (regexOn) {
      try {
        const re = new RegExp(filter, "i");
        return { rows: lines.filter((l) => re.test(l.message)), bad: false };
      } catch {
        return { rows: lines, bad: true };
      }
    }
    const needle = filter.toLowerCase();
    return {
      rows: lines.filter((l) => l.message.toLowerCase().includes(needle)),
      bad: false,
    };
  });
  const shown = $derived(filtered.rows);
  const regexBad = $derived(filtered.bad);
  const errorCount = $derived(shown.filter((l) => severityOf(l.message) === "ERROR").length);
  const warnCount = $derived(shown.filter((l) => severityOf(l.message) === "WARN").length);
  const displayed = $derived(
    errorsOnly ? shown.filter((l) => severityOf(l.message) === "ERROR") : shown,
  );

  function jumpToError(dir: "first" | "last") {
    const errs = displayed.filter((l) => severityOf(l.message) === "ERROR");
    if (errs.length === 0) return;
    selectedLine = dir === "first" ? errs[0] : errs[errs.length - 1];
    gotoMs = selectedLine.timestampMs;
    setTimeout(() => (gotoMs = null), 60);
  }

  function openFirstErrorTrace() {
    const line = displayed.find(
      (l) =>
        severityOf(l.message) === "ERROR" &&
        traceIdsForLine(l.message, l.traceIds).length > 0,
    );
    if (!line) return;
    openTrace(traceIdsForLine(line.message, line.traceIds)[0]);
  }

  function parseJsonMessage(msg: string): unknown | null {
    const t = msg.trim();
    if (!t.startsWith("{") && !t.startsWith("[")) return null;
    try {
      return JSON.parse(t);
    } catch {
      return null;
    }
  }

  const selectedJson = $derived(
    selectedLine ? parseJsonMessage(selectedLine.message) : null,
  );
  const selectedPretty = $derived(
    selectedJson != null
      ? JSON.stringify(selectedJson, null, 2)
      : (selectedLine?.message ?? ""),
  );

  /** Tree: only log-emitting constructs; hide LogGroups already owned by a Lambda/etc. */
  const viewNodes = $derived.by(() => {
    const covered = new Set(
      nodes.filter((n) => n.kind !== "log-group" && n.log_group).map((n) => n.log_group!),
    );
    return nodes.filter((n) => {
      if (!n.log_group) return false;
      if (n.kind === "log-group" && covered.has(n.log_group)) return false;
      return true;
    });
  });

  const stacks = $derived.by(() => {
    const m = new Map<string, ResourceNode[]>();
    for (const n of viewNodes) {
      const [stack] = n.construct_path.split("/");
      if (!m.has(stack)) m.set(stack, []);
      m.get(stack)!.push(n);
    }
    return [...m.entries()];
  });
  const filteredStacks = $derived(
    treeFilter
      ? stacks
          .map(
            ([s, ns]) =>
              [
                s,
                ns.filter((n) => n.construct_path.toLowerCase().includes(treeFilter.toLowerCase())),
              ] as const,
          )
          .filter(([, ns]) => ns.length > 0)
      : stacks,
  );

  function shortName(path: string): string {
    const parts = path.split("/");
    return parts.slice(1).join("/") || path;
  }

  function shortKind(kind: string): string {
    return (
      {
        "lambda-function": "LAMBDA",
        "log-group": "LOGS",
        "api-stage": "STAGE",
        "rest-api": "RESTAPI",
        "http-api": "HTTPAPI",
        "ecs-task-definition": "ECS",
        "ecs-service": "ECS",
        "state-machine": "SFN",
        "dynamo-db-table": "DDB",
        "sqs-queue": "SQS",
        "sns-topic": "SNS",
        "s3-bucket": "S3",
        "kinesis-stream": "KINESIS",
        "event-bridge-rule": "EVENT",
      }[kind] ?? kind.toUpperCase().slice(0, 8)
    );
  }

  const byLogGroup = $derived.by(() => {
    const m = new Map<string, string>();
    for (const n of nodes) {
      if (n.log_group && n.kind !== "log-group") m.set(n.log_group, n.construct_path);
    }
    // Prefer non-log-group owners; fall back to any owner.
    for (const n of nodes) {
      if (n.log_group && !m.has(n.log_group)) m.set(n.log_group, n.construct_path);
    }
    return m;
  });

  const mapStackFilter = $derived.by(() => {
    if (!selected) return null;
    return selected.split("/")[0] ?? null;
  });

  const tracePath = $derived(
    trace ? constructsAlongTrace(trace, nodes) : ([] as string[]),
  );

  function bumpHeat(batch: LogLine[]) {
    if (batch.length === 0) return;
    const next = { ...heat };
    const t = Date.now();
    for (const line of batch) {
      const path = byLogGroup.get(line.logGroup);
      if (!path) continue;
      const sev = severityOf(line.message);
      const prev = next[path];
      const baseErr = prev && t - prev.lastPulseMs > 10_000 ? 0 : (prev?.errCount ?? 0);
      const keepErr =
        prev?.severity === "ERROR" && t - (prev?.lastPulseMs ?? 0) < 8000 ? "ERROR" : sev;
      next[path] = {
        lastPulseMs: t,
        severity: sev === "ERROR" || sev === "WARN" ? sev : keepErr,
        errCount: sev === "ERROR" ? baseErr + 1 : baseErr,
        lastError: sev === "ERROR" ? line.message.slice(0, 120) : prev?.lastError,
      };
    }
    heat = next;
  }

  async function verifyGroups() {
    if (!stage || verified) return;
    verified = true;
    try {
      const res = await api.verifyLogGroups(
        { profile: stage.profile, region: regionOf(stage) },
        stage.cdkOut,
      );
      verifiedMissing = res.missing;
      if (res.missing.length > 0 && banner?.kind !== "stale" && banner?.kind !== "auth") {
        banner = {
          kind: "missing",
          title: `${res.missing.length} log group${res.missing.length === 1 ? "" : "s"} not in CloudWatch`,
          hint: "Undeployed stack, wrong region/profile, or never invoked.",
          group: res.missing[0],
        };
      }
    } catch (e) {
      // Auth failures surface via search; don't block the tree.
      const f = friendlyAwsError(e, awsCtx());
      if (f.kind === "auth") setAwsBanner(e);
    }
  }

  async function runSynth() {
    if (!stage || synthesizing) return;
    const proj = store.projects.find((p) => p.id === stage.projectId);
    if (!proj) {
      banner = {
        kind: "error",
        title: "No project folder",
        hint: "Open a stage linked to a CDK project to run synth.",
        raw: "",
      };
      return;
    }
    synthesizing = true;
    try {
      const res = await api.runSynth(proj.path);
      if (res.outdir && res.outdir !== stage.cdkOut) {
        store.updateStage(stage.id, { cdkOut: res.outdir });
      }
      banner = null;
      staleChecked = false;
      verified = false;
      await loadGraph();
      await search();
    } catch (e) {
      banner = {
        kind: "error",
        title: "cdk synth failed",
        hint: String(e),
        raw: String(e),
      };
    } finally {
      synthesizing = false;
    }
  }

  async function loadGraph() {
    if (!stage) return;
    loading = true;
    banner = null;
    try {
      const graph = await api.getGraph(stage.cdkOut);
      nodes = graph.nodes;
      edges = graph.edges ?? [];
      heat = {};
      const covered = new Set(
        nodes.filter((n) => n.kind !== "log-group" && n.log_group).map((n) => n.log_group!),
      );
      const visible = nodes.filter(
        (n) => n.log_group && !(n.kind === "log-group" && covered.has(n.log_group)),
      );
      onNodes(visible);
      if (!selected || !visible.some((n) => n.construct_path === selected)) {
        selected =
          visible.find((n) => n.kind === "lambda-function")?.construct_path ??
          visible[0]?.construct_path ??
          null;
      }
      // Staleness (needs the project dir, not just cdk.out).
      const proj = store.projects.find((p) => p.id === stage.projectId);
      if (proj && !staleChecked) {
        staleChecked = true;
        const info = await api.inspectCdkOut(proj.path, stage.cdkOut);
        if (info.stale) {
          banner = {
            kind: "stale",
            title: "cdk.out may be stale",
            hint: "Local synth is older than your sources — results can be out of date.",
          };
        }
      }
      void verifyGroups();
    } catch (e) {
      setAwsBanner(e);
    } finally {
      loading = false;
    }
  }

  async function search(opts?: { startMs?: number; endMs?: number }) {
    if (!stage || !selected) return;
    loading = true;
    persistPrefs();
    // Constructs without log groups (IAM roles, etc.) never need a backend call.
    // Stack headers still match via path-prefix (selectionHasLogs).
    if (!selectionHasLogs) {
      lines = [];
      missing = [];
      nextCursor = null;
      selectedLine = null;
      if (tailing) await stopTail();
      loading = false;
      return;
    }
    try {
      const win =
        opts?.startMs != null
          ? { startMs: opts.startMs, endMs: opts.endMs }
          : windowArgs();
      const res = await api.searchLogs({
        connection: { profile: stage.profile, region: regionOf(stage) },
        cdkOut: stage.cdkOut,
        constructPath: selected,
        back: win.startMs != null ? undefined : (win.back ?? back),
        start: win.startMs != null ? String(win.startMs) : undefined,
        end: win.endMs != null ? String(win.endMs) : undefined,
        // Regex mode is client-side only: CloudWatch syntax isn't regex.
        filter: !regexOn && filter ? filter : undefined,
        limit: 500,
      });
      lines = res.lines;
      bumpHeat(res.lines);
      missing = res.missing;
      nextCursor = res.nextCursor ?? null;
      if (filter.trim()) rememberFilter(stage.id, filter);
      if (missing.length > 0) {
        banner = {
          kind: "missing",
          title: "Log group not created yet",
          hint: "It usually appears after the first Lambda invoke. Deploy the stack if this never shows up.",
          group: missing[0],
        };
      } else if (banner?.kind === "missing") {
        banner = null;
      }
      selectedLine = null;
      if (tailing) await stopTail();
    } catch (e) {
      setAwsBanner(e);
    } finally {
      loading = false;
    }
  }

  async function toggleTail() {
    if (!stage || !selected) return;
    if (tailing && !paused) {
      // Pause: stop streaming, keep the buffer and tail mode.
      try {
        await api.stopTail();
      } catch {
        // Already stopped.
      }
      paused = true;
      return;
    }
    await startTail();
  }

  async function startTail() {
    if (!stage || !selected) return;
    try {
      const win = windowArgs();
      await api.startTail(
        {
          connection: { profile: stage.profile, region: regionOf(stage) },
          cdkOut: stage.cdkOut,
          constructPath: selected,
          back: win.startMs != null ? undefined : (win.back ?? back),
          start: win.startMs != null ? String(win.startMs) : undefined,
          end: win.endMs != null ? String(win.endMs) : undefined,
          filter: filter || undefined,
          limit: 100,
        },
        store.settings.tailIntervalSecs,
      );
      tailing = true;
      paused = false;
      autoScroll = true;
    } catch (e) {
      setAwsBanner(e);
    }
  }

  async function stopTail() {
    try {
      await api.stopTail();
    } catch {
      // Already stopped.
    }
    tailing = false;
    paused = false;
  }

  /** Page forward in the current result set (FR-12). */
  async function loadMore() {
    if (!stage || !selected || !nextCursor || loadingMore) return;
    loadingMore = true;
    try {
      const win = windowArgs();
      const res = await api.searchLogs({
        connection: { profile: stage.profile, region: regionOf(stage) },
        cdkOut: stage.cdkOut,
        constructPath: selected,
        back: win.startMs != null ? undefined : (win.back ?? back),
        start: win.startMs != null ? String(win.startMs) : undefined,
        end: win.endMs != null ? String(win.endMs) : undefined,
        filter: !regexOn && filter ? filter : undefined,
        limit: 500,
        cursor: nextCursor,
      });
      const key = (l: LogLine) => `${l.timestampMs}|${l.logStream}|${l.message}`;
      const seen = new Set(lines.map(key));
      const fresh = res.lines.filter((l) => !seen.has(key(l)));
      lines = [...lines, ...fresh]
        .sort((a, b) => a.timestampMs - b.timestampMs)
        .slice(-store.settings.bufferLimit);
      missing = res.missing;
      nextCursor = res.nextCursor ?? null;
    } catch (e) {
      setAwsBanner(e);
    } finally {
      loadingMore = false;
    }
  }

  function applyTraceResult(out: { trace: unknown }) {
    const t = out.trace as XRayTrace | null;
    if (!t || !Array.isArray(t.segments)) {
      trace = null;
      traceErr = "Trace response was empty or malformed.";
      traceHint = "Try another ID, or check the stage region.";
    } else {
      trace = t;
      traceErr = "";
    }
  }

  async function openTrace(id: string) {
    if (!stage) return;
    const tid = id.trim();
    if (!tid) return;
    traceErr = "";
    traceHint = "";
    traceCmd = undefined;
    traceLoading = true;
    try {
      const out = await api.fetchTrace(
        { profile: stage.profile, region: regionOf(stage) },
        tid,
        stage.cdkOut,
      );
      applyTraceResult(out);
    } catch (e) {
      trace = null;
      const f = friendlyAwsError(e, awsCtx());
      traceErr = f.title;
      traceHint = f.hint;
      traceCmd = f.command;
    } finally {
      traceLoading = false;
    }
  }

  /** Fast path: same RequestId already co-occurs with a trace id in the buffer. */
  function localTraceForRequest(requestId: string): string | null {
    for (const l of lines) {
      if (!l.message.includes(requestId)) continue;
      const ids = (l.traceIds ?? []).filter(Boolean);
      if (ids.length > 0) return ids[0];
      const m = l.message.match(/1-[0-9a-fA-F]{8}-[0-9a-fA-F]{24}/);
      if (m) return m[0];
    }
    return null;
  }

  async function openRequestId(requestId: string, aroundMs: number) {
    if (!stage) return;
    const rid = requestId.trim();
    if (!rid) return;
    traceErr = "";
    traceHint = "";
    traceCmd = undefined;
    traceLoading = true;
    try {
      const local = localTraceForRequest(rid);
      if (local) {
        const out = await api.fetchTrace(
          { profile: stage.profile, region: regionOf(stage) },
          local,
          stage.cdkOut,
        );
        applyTraceResult(out);
        return;
      }
      const out = await api.resolveRequestTrace(
        { profile: stage.profile, region: regionOf(stage) },
        rid,
        aroundMs,
        back,
      );
      applyTraceResult(out);
    } catch (e) {
      trace = null;
      const f = friendlyAwsError(e, awsCtx());
      traceErr = f.title;
      traceHint = f.hint || String(e);
      traceCmd = f.command;
    } finally {
      traceLoading = false;
    }
  }

  /** Clear the on-screen buffer only — never touches CloudWatch. */
  function clearLines() {
    lines = [];
    selectedLine = null;
  }

  // Reload when the active stage changes; stop tail on switch/unmount.
  let lastStageId: string | null = null;
  $effect(() => {
    const id = store.activeStageId;
    if (id && id !== lastStageId) {
      lastStageId = id;
      staleChecked = false;
      verified = false;
      missing = [];
      verifiedMissing = [];
      trace = null;
      lines = [];
      regionOverride = null;
      restorePrefs(id);
      applyPendingHandoff();
      stopTail().then(loadGraph).then(() => search());
    }
    return () => {};
  });

  /** WS7: apply deep-link filter/window — returns true when something applied. */
  function applyPendingHandoff(): boolean {
    const f = store.pendingFilter;
    const w = store.pendingWindow;
    if (f == null && w == null) return false;
    store.pendingFilter = null;
    store.pendingWindow = null;
    if (f != null) {
      filter = f;
      const sid = store.activeStageId;
      if (sid) rememberFilter(sid, f);
    }
    if (w) {
      useCustom = true;
      customStart = toLocalInput(w.startMs);
      customEnd = toLocalInput(w.endMs);
    }
    return true;
  }

  function toLocalInput(ms: number): string {
    const d = new Date(ms);
    const p = (n: number) => String(n).padStart(2, "0");
    return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  }

  // Same-stage deep links: the stage effect above won't re-run, so apply
  // the handoff once the graph for the current stage is loaded.
  $effect(() => {
    const f = store.pendingFilter;
    const w = store.pendingWindow;
    if ((f != null || w != null) && lastStageId && lastStageId === store.activeStageId && nodes.length > 0) {
      if (applyPendingHandoff()) search();
    }
  });

  // ⌘K open-trace handoff.
  $effect(() => {
    const tid = store.pendingTraceId;
    if (tid && store.view === "debugger" && store.activeStageId) {
      store.pendingTraceId = null;
      void openTrace(tid);
    }
  });

  onMount(() => {
    (async () => {
      unlistens = [
        await onLogBatch((batch) => {
          lines = [...lines, ...batch].slice(-store.settings.bufferLimit);
          bumpHeat(batch);
        }),
        await onLogError((msg) => {
          setAwsBanner(msg);
        }),
      ];
      if (store.activeStageId) {
        lastStageId = store.activeStageId;
        restorePrefs(store.activeStageId);
        await loadGraph();
        await search();
      }
    })();
    return () => {
      unlistens.forEach((u) => u());
      void stopTail();
    };
  });

  // External construct selection (e.g. from ⌘K): consume the pending request.
  $effect(() => {
    const p = store.pendingConstruct;
    if (p && nodes.some((n) => n.construct_path === p)) {
      store.pendingConstruct = null;
      selected = p;
      search();
    }
  });

  export function selectConstruct(path: string) {
    selected = path;
    search();
  }
</script>

{#if !stage}
  <div class="empty">Pick a stage from the sidebar to start debugging.</div>
{:else}
  <div class="ctx">
    {#if banner}
      <div class="banner {banner.kind}">
        <span class="material-symbols-outlined"
          >{banner.kind === "auth" ? "lock" : banner.kind === "stale" ? "schedule" : "warning"}</span
        >
        <div class="btext">
          <div class="btitle">{banner.title}</div>
          {#if banner.hint}
            <div class="bhint">{banner.hint}</div>
          {/if}
          {#if banner.kind === "missing" && banner.group}
            <code class="bcode">{banner.group}</code>
          {/if}
          {#if banner.kind === "auth" || banner.kind === "error"}
            {@const cmd = banner.command}
            {@const raw = banner.raw}
            {#if cmd}
              <div class="bcmd">
                <code class="mono">{cmd}</code>
                <button type="button" class="btn btn-ghost btn-sm" onclick={() => copyCommand(cmd)}>
                  {cmdCopied ? "Copied" : "Copy"}
                </button>
                <button
                  type="button"
                  class="btn btn-primary btn-sm"
                  onclick={() => openTerminalLogin(cmd)}
                >
                  Copy &amp; open Terminal
                </button>
              </div>
            {/if}
            {#if raw}
              <button type="button" class="details-toggle" onclick={() => (bannerDetails = !bannerDetails)}>
                {bannerDetails ? "Hide details" : "Show details"}
              </button>
              {#if bannerDetails}
                <pre class="braw mono">{raw}</pre>
              {/if}
            {/if}
          {/if}
        </div>
        <span class="spacer"></span>
        {#if banner.kind === "stale"}
          <button
            type="button"
            class="btn btn-primary btn-sm"
            disabled={synthesizing}
            onclick={runSynth}
          >
            {synthesizing ? "Synthesizing…" : "Synthesize now"}
          </button>
        {/if}
        {#if banner.kind === "auth" || banner.kind === "missing" || banner.kind === "error"}
          <button type="button" class="btn btn-ghost btn-sm" onclick={() => search()}>Retry</button>
        {/if}
        <button type="button" class="btn btn-ghost btn-sm" onclick={() => (banner = null)}>✕</button>
      </div>
    {/if}
    <div class="ctx-row">
      <button type="button" class="ctx-switch" onclick={() => (store.view = "dashboard")}>
        <strong>{store.projectName(stage.projectId)}</strong>
        <span class="dim">/</span> {stage.name}
      </button>
      <span class="dim mono ctx-meta">
        {stage.profile ?? "default"} ·
        <select
          class="region-select mono"
          bind:value={regionOverride}
          onchange={() => {
            nextCursor = null;
            search();
          }}
          title="Region for this session (override the stage region)"
        >
          <option value={null}>{stage.region ?? "(default)"}</option>
          {#each REGION_CHOICES.filter((r) => r !== stage.region) as r}
            <option value={r}>{r}</option>
          {/each}
        </select>
      </span>
      <button type="button" class="btn btn-ghost btn-sm" title="Reveal in Finder" onclick={revealProject}
        >Finder</button
      >
      <button type="button" class="btn btn-ghost btn-sm" title="CloudWatch console" onclick={openCwConsole}
        >CW</button
      >
      <button type="button" class="btn btn-ghost btn-sm" title="X-Ray console" onclick={openXrayConsole}
        >X-Ray</button
      >
      <span class="spacer"></span>
      {#each PRESETS as p (p)}
        <button
          type="button"
          class="preset mono"
          class:on={!useCustom && back === p}
          onclick={() => {
            useCustom = false;
            back = p;
            persistPrefs();
            search();
          }}>{p}</button
        >
      {/each}
      <button
        type="button"
        class="preset mono"
        class:on={useCustom}
        onclick={() => (useCustom = !useCustom)}
        >Custom</button
      >
    </div>
    {#if useCustom}
      <div class="ctx-row custom-range">
        <label class="dim" for="range-start">From</label>
        <input id="range-start" type="datetime-local" class="input mono" bind:value={customStart} />
        <label class="dim" for="range-end">To</label>
        <input id="range-end" type="datetime-local" class="input mono" bind:value={customEnd} />
        <button
          type="button"
          class="btn btn-primary btn-sm"
          disabled={!customStart || !customEnd}
          onclick={() => {
            persistPrefs();
            search();
          }}>Apply</button
        >
      </div>
    {/if}
    <div class="ctx-row">
      <div class="search">
        <span class="material-symbols-outlined">search</span>
        <input
          bind:value={filter}
          placeholder="Filter logs…"
          onkeydown={(e) => e.key === "Enter" && search()}
          list="saved-filters"
        />
        <datalist id="saved-filters">
          {#each savedFilters as f (f)}
            <option value={f}></option>
          {/each}
        </datalist>
        {#if regexBad}<span class="badge badge-err">bad regex</span>{/if}
        <button
          type="button"
          class="toggle mono"
          class:on={regexOn}
          title="Regex"
          onclick={() => (regexOn = !regexOn)}>.*</button
        >
        {#if filter}
          <button type="button" class="toggle mono" onclick={() => (filter = "")}>Clear</button>
        {/if}
      </div>
      <button
        type="button"
        class="btn btn-ghost btn-sm"
        title="Run Logs Insights (costs apply)"
        disabled={!selected || !selectionHasLogs}
        onclick={() => {
          insightsOpen = true;
          insightsErr = "";
        }}>Insights</button
      >
      <button
        type="button"
        class="btn btn-ghost btn-sm"
        title="Copy shareable context"
        onclick={() => copyText(contextSnippet(), "Context copied")}
        >Export</button
      >
      <button
        type="button"
        class="btn btn-ghost btn-sm"
        title="Copy a cw-trace:// link for this view"
        onclick={copyDeepLink}>Link</button
      >
      <button
        type="button"
        class="btn btn-ghost btn-sm"
        class:live={tailing && !paused}
        onclick={toggleTail}
      >
        <span class="dot {tailing && !paused ? 'dot-live' : 'dot-unknown'}"></span>
        {tailing ? (paused ? "Resume" : "Pause") : "Tail"}
      </button>
    </div>
  </div>

  <div
    class="panes"
    class:resizing={dragging !== null}
    style={`--tree-w: ${treeW}px; --trace-w: ${traceW}px`}
  >
    <section class="tree">
      <div class="pane-head">
        <span class="label">Resources</span>
        <span class="dim mono" style="font-size: 11px">{viewNodes.length}</span>
        <span class="spacer"></span>
        <div class="mode-toggle" role="group" aria-label="Resource view">
          <button
            type="button"
            class="toggle mono"
            class:on={treeMode === "list"}
            onclick={() => (treeMode = "list")}>List</button
          >
          <button
            type="button"
            class="toggle mono"
            class:on={treeMode === "map"}
            onclick={() => (treeMode = "map")}>Map</button
          >
        </div>
      </div>
      {#if treeMode === "list"}
        <div class="search small">
          <span class="material-symbols-outlined">search</span>
          <input bind:value={treeFilter} placeholder="Filter…" />
        </div>
        <div class="tree-body">
          {#each filteredStacks as [stack, ns] (stack)}
            <button
              type="button"
              class="stack"
              class:sel={selected === stack}
              title="Tail all log groups under this stack"
              onclick={() => {
                selected = stack;
                search();
              }}
            >
              <span class="material-symbols-outlined">account_tree</span> {stack}
              <span class="dim stack-hint">all</span>
            </button>
            {#each ns as n (n.construct_path)}
              {@const isMissing =
                n.log_group !== undefined &&
                n.log_group !== null &&
                (verifiedMissing.includes(n.log_group) || missing.includes(n.log_group))}
              <button
                type="button"
                class="node"
                class:sel={selected === n.construct_path}
                class:pending={isMissing}
                title={n.log_group ?? ""}
                onclick={() => {
                  selected = n.construct_path;
                  search();
                }}
              >
                <span
                  class="dot {isMissing ? 'dot-err' : 'dot-ok'}"
                  title={n.log_group ?? ""}
                ></span>
                <span class="nname mono">{shortName(n.construct_path)}</span>
                <span class="badge badge-dim">{shortKind(n.kind)}</span>
              </button>
            {/each}
          {/each}
          {#if viewNodes.length === 0 && !loading}
            <p class="dim empty-tree">No log-emitting resources found.</p>
          {/if}
        </div>
      {:else}
        <ConstructMap
          {nodes}
          {edges}
          {selected}
          {heat}
          {tracePath}
          stackFilter={mapStackFilter}
          onSelect={(path) => {
            selected = path;
            search();
          }}
          onSelectSpan={(name) => {
            focusSpanName = name;
            // Allow re-selecting the same name later.
            setTimeout(() => {
              if (focusSpanName === name) focusSpanName = null;
            }, 50);
          }}
        />
      {/if}
    </section>

    <button
      type="button"
      class="splitter"
      aria-label="Resize resources pane"
      onpointerdown={(e) => startResize("tree", e)}
      onkeydown={(e) => {
        if (e.key === "ArrowLeft") {
          treeW = clamp(treeW - 16, TREE_MIN, TREE_MAX);
          savePaneWidth("tree", treeW);
        } else if (e.key === "ArrowRight") {
          treeW = clamp(treeW + 16, TREE_MIN, TREE_MAX);
          savePaneWidth("tree", treeW);
        }
      }}
    ></button>

    <section class="logs">
      <div class="pane-head">
        <span class="label">Logs</span>
        {#if selectedNode && selected}
          <span class="dim mono sel-path" title={selectedNode.log_group ?? selected}
            >{shortName(selected)}</span
          >
        {/if}
        <span class="spacer"></span>
        {#if errorCount > 0 || warnCount > 0}
          <button
            type="button"
            class="toggle mono err-toggle"
            class:on={errorsOnly}
            title="Show errors only"
            onclick={() => (errorsOnly = !errorsOnly)}
            >{errorCount} err</button
          >
          {#if warnCount > 0}
            <span class="badge badge-warn mono">{warnCount} warn</span>
          {/if}
          <button
            type="button"
            class="btn btn-ghost btn-sm"
            title="Jump to first error"
            onclick={() => jumpToError("first")}>↑ first</button
          >
          <button
            type="button"
            class="btn btn-ghost btn-sm"
            title="Jump to last error"
            onclick={() => jumpToError("last")}>↓ last</button
          >
          <button
            type="button"
            class="btn btn-ghost btn-sm"
            title="Open the first error line's trace"
            onclick={openFirstErrorTrace}>trace err</button
          >
        {/if}
        <span class="dim mono" style="font-size: 11px">
          {errorsOnly ? `${displayed.length} of ${shown.length}` : shown.length}
        </span>
        <button
          type="button"
          class="btn btn-ghost btn-sm"
          disabled={!selectedLine}
          onclick={() =>
            selectedLine && copyText(selectedLine.message, "Line copied")}
          >Copy line</button
        >
        <button
          type="button"
          class="btn btn-ghost btn-sm"
          disabled={shown.length === 0}
          onclick={() =>
            copyText(
              shown
                .slice(0, 200)
                .map((l) => `${l.timestamp}\t${l.message}`)
                .join("\n"),
              "Lines copied",
            )}
          >Copy shown</button
        >
        <button type="button" class="btn btn-ghost btn-sm" onclick={clearLines}>Clear</button>
        {#if nextCursor}
          <button
            type="button"
            class="btn btn-ghost btn-sm"
            title="Page forward in the selected window"
            disabled={loadingMore}
            onclick={loadMore}>{loadingMore ? "Loading…" : "Load newer"}</button
          >
        {/if}
        <button
          type="button"
          class="toggle mono"
          class:on={autoScroll}
          title="Auto-scroll to newest lines while tailing"
          onclick={() => (autoScroll = !autoScroll)}>follow</button
        >
      </div>
      {#if selected && !selectionHasLogs}
        <div class="poll-empty">
          <p class="dim">This construct has no CloudWatch logs. Pick a Lambda or API stage.</p>
        </div>
      {:else if missing.length > 0 && lines.length === 0}
        <div class="poll-empty">
          <h3>No logs yet</h3>
          <p class="dim">
            <span class="mono">{missing[0]}</span> appears after the first invocation.
          </p>
          <div class="row-btns">
            <button
              type="button"
              class="btn btn-ghost btn-sm"
              onclick={() => {
                back = "24h";
                search();
              }}>Last 24h</button
            >
            {#if !tailing}
              <button type="button" class="btn btn-ghost btn-sm" onclick={toggleTail}>Start tail</button>
            {/if}
          </div>
        </div>
      {:else}
        <div class="table-wrap">
          <VirtualLogTable
            lines={displayed}
            gotoMs={gotoMs}
            autoScroll={autoScroll}
            onUserScrollUp={() => (autoScroll = false)}
            selectedMs={selectedLine?.timestampMs ?? null}
            onSelect={(l) => (selectedLine = l)}
            onOpenTrace={openTrace}
            onOpenRequestId={openRequestId}
          />
        </div>
      {/if}
      {#if selectedLine}
        <div class="detail card">
          <div class="detail-head">
            <span class="label">Line</span>
            <span class="mono dim detail-src"
              >{selectedLine.logGroup}/{selectedLine.logStream}</span
            >
            <span class="detail-spacer"></span>
            {#if selectedJson != null}<span class="badge badge-dim mono">json</span>{/if}
            <button
              type="button"
              class="btn btn-ghost btn-sm"
              onclick={() => copyText(selectedLine!.message)}
              >Copy</button
            >
            {#if selectedJson != null}
              <button
                type="button"
                class="btn btn-ghost btn-sm"
                onclick={() => copyText(selectedPretty, "Copied JSON")}
                >Copy JSON</button
              >
            {/if}
          </div>
          <pre class="detail-body mono">{selectedPretty}</pre>
        </div>
      {/if}
    </section>

    <button
      type="button"
      class="splitter"
      aria-label="Resize trace pane"
      onpointerdown={(e) => startResize("trace", e)}
      onkeydown={(e) => {
        if (e.key === "ArrowLeft") {
          traceW = clamp(traceW + 16, TRACE_MIN, TRACE_MAX);
          savePaneWidth("trace", traceW);
        } else if (e.key === "ArrowRight") {
          traceW = clamp(traceW - 16, TRACE_MIN, TRACE_MAX);
          savePaneWidth("trace", traceW);
        }
      }}
    ></button>

    <section class="tracepane">
      <TracePanel
        {trace}
        loading={traceLoading}
        error={traceErr}
        errorHint={traceHint}
        errorCommand={traceCmd}
        region={regionOf(stage)}
        profile={stage.profile}
        back={useCustom ? undefined : back}
        customStartMs={useCustom && customStart ? Date.parse(customStart) : undefined}
        customEndMs={useCustom && customEnd ? Date.parse(customEnd) : undefined}
        serviceHint={selectedNode?.function_name}
        nodes={nodes}
        {focusSpanName}
        onOpenTrace={openTrace}
        onViewLogs={(s, e, constructPath) => {
          if (tailing) stopTail();
          if (constructPath) selected = constructPath;
          search({ startMs: s, endMs: e });
        }}
      />
    </section>
  </div>

  {#if insightsOpen}
    <div
      class="insights-overlay"
      role="dialog"
      aria-modal="true"
      aria-label="Logs Insights"
      tabindex="-1"
      onclick={() => (insightsOpen = false)}
      onkeydown={(e) => e.key === "Escape" && (insightsOpen = false)}
    >
      <div class="insights-card" role="document" onclick={(e) => e.stopPropagation()}>
        <div class="insights-head">
          <strong>Logs Insights</strong>
          <span class="dim">Explicit run — billed by AWS</span>
          <span class="spacer"></span>
          <button type="button" class="btn btn-ghost btn-sm" onclick={() => (insightsOpen = false)}
            >Close</button
          >
        </div>
        <textarea class="input mono insights-q" rows="4" bind:value={insightsQuery}></textarea>
        <div class="insights-actions">
          <button
            type="button"
            class="btn btn-primary btn-sm"
            disabled={insightsBusy || !selected}
            onclick={runInsights}
          >
            {insightsBusy ? "Running…" : "Run Insights"}
          </button>
          {#if insightsErr}<span class="badge badge-err">{insightsErr}</span>{/if}
        </div>
        {#if insightsRows.length > 0}
          <div class="insights-table mono">
            {#each insightsRows as row, i (i)}
              <div class="insights-row">
                {#each Object.entries(row) as [k, v] (`${i}-${k}`)}
                  <span><em>{k}</em> {v}</span>
                {/each}
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  {/if}
  {#if toast}<div class="dbg-toast mono" role="status">{toast}</div>{/if}
{/if}

<style>
  .empty {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--text-faint);
  }
  .ctx {
    border-bottom: 1px solid var(--border);
    background: var(--bg1);
    flex: none;
  }
  .banner {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 14px;
    font-size: 12px;
    border-bottom: 1px solid var(--border);
  }
  .banner > .material-symbols-outlined {
    margin-top: 1px;
    flex: none;
  }
  .banner.stale {
    background: rgba(249, 115, 22, 0.08);
    color: var(--amber);
  }
  .banner.auth,
  .banner.error,
  .banner.missing {
    background: rgba(224, 112, 112, 0.1);
    color: var(--red-bright);
  }
  .btext {
    min-width: 0;
    flex: 1;
  }
  .btitle {
    font-weight: 600;
    font-size: 13px;
    color: var(--text);
  }
  .bhint {
    margin-top: 3px;
    color: var(--text-dim);
    line-height: 1.45;
  }
  .bcode {
    display: inline-block;
    margin-top: 6px;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--amber);
    background: rgba(249, 115, 22, 0.1);
    padding: 2px 6px;
    border-radius: 4px;
  }
  .bcmd {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 8px;
    flex-wrap: wrap;
  }
  .bcmd code {
    font-size: 11px;
    background: var(--bg0);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: 4px;
    padding: 4px 8px;
  }
  .details-toggle {
    display: block;
    margin-top: 6px;
    background: none;
    border: none;
    color: var(--text-faint);
    font-size: 11px;
    padding: 0;
    cursor: pointer;
    text-decoration: underline;
  }
  .braw {
    margin: 6px 0 0;
    padding: 8px;
    max-height: 90px;
    overflow: auto;
    font-size: 10px;
    line-height: 1.4;
    white-space: pre-wrap;
    word-break: break-word;
    background: var(--bg0);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-faint);
  }
  .ctx-meta {
    font-size: 11px;
  }
  .region-select {
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--text-faint);
    font-size: 11px;
    padding: 1px 2px;
    cursor: pointer;
  }
  .region-select:hover,
  .region-select:focus {
    border-color: var(--border);
    color: var(--text);
    outline: none;
  }
  .ctx-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
  }
  .ctx-switch {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg2);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: 6px;
    padding: 6px 12px;
    font-size: 13px;
  }
  .spacer {
    flex: 1;
  }
  .preset {
    background: none;
    border: 1px solid transparent;
    color: var(--text-dim);
    border-radius: 4px;
    font-size: 11px;
    padding: 4px 8px;
  }
  .preset:hover {
    color: var(--text);
  }
  .preset.on {
    background: var(--bg2);
    border-color: var(--border);
    color: var(--text);
  }
  .search {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg2);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0 10px;
    height: 32px;
  }
  .search.small {
    height: 30px;
    margin: 8px 10px 4px;
    flex: none;
  }
  .search input {
    flex: 1;
    background: none;
    border: none;
    outline: none;
    color: var(--text);
    font-size: 12px;
  }
  .search .material-symbols-outlined {
    font-size: 16px;
    color: var(--text-faint);
  }
  .toggle {
    flex: none;
    background: var(--bg0);
    border: 1px solid var(--border);
    color: var(--text-faint);
    border-radius: 4px;
    font-size: 10px;
    padding: 3px 8px;
  }
  .toggle:hover {
    color: var(--text);
  }
  .toggle.on {
    color: var(--sage);
    border-color: rgba(56, 189, 248, 0.4);
    background: rgba(56, 189, 248, 0.08);
  }
  .err-toggle.on {
    color: var(--red-bright, #e07070);
    border-color: rgba(224, 112, 112, 0.5);
    background: rgba(224, 112, 112, 0.1);
  }
  .live {
    border-color: rgba(16, 185, 129, 0.4) !important;
    color: var(--green-bright) !important;
  }
  .panes {
    flex: 1;
    display: grid;
    grid-template-columns: var(--tree-w, 280px) 5px minmax(0, 1fr) 5px var(--trace-w, 360px);
    min-height: 0;
  }
  .panes.resizing {
    cursor: col-resize;
    user-select: none;
  }
  .panes.resizing * {
    cursor: col-resize !important;
    user-select: none !important;
  }
  .splitter {
    position: relative;
    z-index: 2;
    width: 5px;
    margin: 0 -2px;
    padding: 0;
    border: none;
    background: transparent;
    cursor: col-resize;
    touch-action: none;
  }
  .splitter::after {
    content: "";
    position: absolute;
    inset: 0 1px;
    background: var(--border);
    transition: background 0.15s;
  }
  .splitter:hover::after,
  .splitter:focus-visible::after,
  .panes.resizing .splitter::after {
    background: var(--amber);
  }
  .splitter:focus-visible {
    outline: none;
  }
  .tree {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    background: var(--bg1);
  }
  .logs {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }
  .tracepane {
    min-height: 0;
    min-width: 0;
    overflow: hidden;
  }
  .pane-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
    flex: none;
    flex-wrap: wrap;
    min-width: 0;
  }
  .mode-toggle {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border-radius: 6px;
    background: var(--bg2);
    border: 1px solid var(--border);
  }
  .mode-toggle .toggle {
    padding: 2px 8px;
    font-size: 10px;
    border-radius: 4px;
    border: none;
    background: transparent;
    color: var(--text-dim);
  }
  .mode-toggle .toggle.on {
    background: var(--bg0);
    color: var(--text);
  }
  .tree-body {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    padding: 4px 6px 12px;
    min-height: 0;
  }
  .tree-body > * {
    flex: none;
  }
  .stack {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    color: var(--text-dim);
    font-weight: 600;
    font-size: 11px;
    padding: 10px 8px 4px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    background: none;
    border: none;
    border-radius: 6px;
    cursor: pointer;
    text-align: left;
  }
  .stack:hover {
    background: var(--hover);
    color: var(--text);
  }
  .stack.sel {
    color: var(--amber);
  }
  .stack-hint {
    margin-left: auto;
    font-size: 9px;
    text-transform: none;
    letter-spacing: 0;
    font-weight: 500;
  }
  .stack .material-symbols-outlined {
    font-size: 14px;
    color: var(--sage);
  }
  .custom-range {
    gap: 8px;
    align-items: center;
  }
  .custom-range .input {
    width: auto;
    flex: 1;
    min-width: 140px;
    font-size: 11px;
  }
  .insights-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    z-index: 55;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 10vh 16px;
  }
  .insights-card {
    width: 640px;
    max-width: 100%;
    max-height: 70vh;
    overflow: auto;
    background: var(--bg1);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 14px 16px;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.45);
  }
  .insights-head {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 10px;
  }
  .insights-q {
    width: 100%;
    resize: vertical;
    font-size: 12px;
  }
  .insights-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 10px 0;
  }
  .insights-table {
    font-size: 11px;
    border-top: 1px solid var(--border);
  }
  .insights-row {
    padding: 8px 0;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .insights-row em {
    color: var(--text-faint);
    font-style: normal;
    margin-right: 6px;
  }
  .dbg-toast {
    position: fixed;
    bottom: 18px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--bg2);
    border: 1px solid var(--border);
    color: var(--text);
    font-size: 12px;
    border-radius: 8px;
    padding: 8px 14px;
    z-index: 70;
  }
  .node {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    border-left: 2px solid transparent;
    color: var(--text);
    padding: 7px 8px;
    border-radius: 0 6px 6px 0;
    cursor: pointer;
    transition: background 0.15s;
  }
  .node:hover {
    background: var(--hover);
  }
  .node.sel {
    background: rgba(249, 115, 22, 0.07);
    border-left-color: var(--amber);
  }
  .nname {
    flex: 1;
    min-width: 0;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .node.pending {
    opacity: 0.85;
  }
  .empty-tree {
    padding: 16px 10px;
    font-size: 12px;
  }
  .sel-path {
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 40%;
  }
  .table-wrap {
    flex: 1;
    min-height: 0;
  }
  .detail {
    max-height: 180px;
    overflow-y: auto;
    margin: 8px 12px;
    padding: 10px 12px;
    font-size: 11px;
    word-break: break-word;
  }
  .detail-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }
  .detail-src {
    font-size: 10px;
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .detail-spacer {
    flex: 1;
  }
  .detail-body {
    margin: 0;
    white-space: pre-wrap;
    font-size: 11px;
    line-height: 1.5;
  }
  .poll-empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    padding: 30px;
    gap: 8px;
  }
  .poll-empty h3 {
    margin: 0;
    font-size: 15px;
  }
  .poll-empty p {
    max-width: 420px;
    font-size: 12px;
    line-height: 1.5;
    margin: 0;
  }
  .row-btns {
    display: flex;
    gap: 10px;
    margin-top: 8px;
  }
</style>
