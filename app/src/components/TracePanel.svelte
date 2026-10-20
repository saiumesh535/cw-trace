<script lang="ts">
  import { api } from "../lib/api";
  import { xrayTraceUrl } from "../lib/consoleLinks";
  import type { ResourceNode, XRaySegment, XRaySubsegment, XRayTrace } from "../lib/types";
  import { isTraceId, matchConstructForSpan } from "../lib/trace";

  let {
    trace,
    loading = false,
    error = "",
    errorHint = "",
    errorCommand,
    region,
    profile,
    back = "1h",
    customStartMs,
    customEndMs,
    serviceHint,
    nodes = [],
    focusSpanName = null,
    onViewLogs,
    onOpenTrace,
  }: {
    trace: XRayTrace | null;
    loading?: boolean;
    error?: string;
    errorHint?: string;
    errorCommand?: string;
    region?: string;
    profile?: string;
    back?: string;
    customStartMs?: number;
    customEndMs?: number;
    serviceHint?: string;
    nodes?: ResourceNode[];
    /** When set, select the first waterfall row whose name matches. */
    focusSpanName?: string | null;
    onViewLogs: (startMs: number, endMs: number, constructPath?: string) => void;
    onOpenTrace: (id: string) => void;
  } = $props();

  interface Row {
    kind: "segment" | "subsegment";
    path: string;
    name: string;
    start_ms: number;
    end_ms: number;
    origin?: string;
    namespace?: string;
    aws_operation?: string;
    http_status?: number;
    error?: boolean;
    fault?: boolean;
    throttle?: boolean;
    cause?: string;
    depth: number;
    hasChildren: boolean;
  }

  let selPath = $state<string | null>(null);
  let collapsed = $state<Record<string, boolean>>({});
  let pasteId = $state("");
  let cmdCopied = $state(false);
  let idCopied = $state(false);
  let summaries = $state<
    { id: string; duration_secs?: number; has_fault: boolean; has_error: boolean }[]
  >([]);
  let summariesBusy = $state(false);
  let summariesErr = $state("");

  async function copyCmd() {
    if (!errorCommand) return;
    try {
      await navigator.clipboard.writeText(errorCommand);
      cmdCopied = true;
      setTimeout(() => (cmdCopied = false), 2000);
    } catch {
      // ignore
    }
  }

  async function copyId() {
    if (!trace) return;
    try {
      await navigator.clipboard.writeText(trace.id);
      idCopied = true;
      setTimeout(() => (idCopied = false), 2000);
    } catch {
      // ignore
    }
  }

  async function openInConsole() {
    if (!trace || !region) return;
    try {
      await api.openExternal(xrayTraceUrl(region, trace.id));
    } catch {
      // ignore
    }
  }

  async function loadSummaries() {
    summariesBusy = true;
    summariesErr = "";
    try {
      summaries = await api.listTraceSummaries({
        connection: { profile, region },
        back: customStartMs != null ? undefined : back,
        start: customStartMs != null ? String(customStartMs) : undefined,
        end: customEndMs != null ? String(customEndMs) : undefined,
        service: serviceHint,
        limit: 25,
      });
      if (summaries.length === 0) summariesErr = "No traces in this window.";
    } catch (e) {
      summaries = [];
      summariesErr = String(e);
    } finally {
      summariesBusy = false;
    }
  }

  function pushSubs(
    subs: XRaySubsegment[] | undefined,
    depth: number,
    parentPath: string,
    out: Row[],
  ) {
    for (const sub of subs ?? []) {
      const path = `${parentPath}/${sub.id}`;
      const children = sub.subsegments ?? [];
      out.push({
        kind: "subsegment",
        path,
        name: sub.name,
        start_ms: sub.start_ms,
        end_ms: sub.end_ms,
        origin: sub.origin,
        namespace: sub.namespace,
        aws_operation: sub.aws_operation,
        http_status: sub.http_status,
        error: sub.error,
        fault: sub.fault,
        throttle: sub.throttle,
        cause: sub.cause,
        depth,
        hasChildren: children.length > 0,
      });
      if (children.length > 0 && !collapsed[path]) {
        pushSubs(children, depth + 1, path, out);
      }
    }
  }

  const rows = $derived.by((): Row[] => {
    const out: Row[] = [];
    for (const s of trace?.segments ?? []) {
      const children = s.subsegments ?? [];
      out.push({
        kind: "segment",
        path: s.id,
        name: s.name,
        start_ms: s.start_ms,
        end_ms: s.end_ms,
        origin: s.origin,
        http_status: s.http_status,
        error: s.error,
        fault: s.fault,
        throttle: s.throttle,
        cause: s.cause,
        depth: 0,
        hasChildren: children.length > 0,
      });
      if (children.length > 0 && !collapsed[s.id]) {
        pushSubs(children, 1, s.id, out);
      }
    }
    return out;
  });

  const spanCount = $derived(
    (trace?.segments ?? []).reduce((acc, s) => {
      const count = (subs: XRaySubsegment[] | undefined): number =>
        (subs ?? []).reduce((a, sub) => a + 1 + count(sub.subsegments), 0);
      return acc + 1 + count(s.subsegments);
    }, 0),
  );

  const sel = $derived(rows.find((r) => r.path === selPath) ?? null);
  const matchedNode = $derived(sel ? matchConstructForSpan(sel.name, nodes) : null);
  const minStart = $derived(trace ? Math.min(...trace.segments.map((s) => s.start_ms)) : 0);
  const maxEnd = $derived(trace ? Math.max(...trace.segments.map((s) => s.end_ms)) : 1);
  const span = $derived(Math.max(maxEnd - minStart, 1));
  const pasteOk = $derived(isTraceId(pasteId.trim()));

  function toggleCollapse(path: string) {
    collapsed = { ...collapsed, [path]: !collapsed[path] };
  }

  function rowFailed(row: Row): boolean {
    return !!(row.fault || row.error || row.throttle);
  }

  function barColor(row: Row): string {
    if (rowFailed(row)) return "var(--red)";
    if (row.namespace === "aws" || /dynamodb|putitem|getitem|query/i.test(row.name)) {
      return "var(--green)";
    }
    if (/lambda|function/i.test(row.name) || row.origin === "AWS::Lambda::Function") {
      return "var(--amber)";
    }
    if (/api|http/i.test(row.name)) return "var(--sage)";
    return "var(--copper)";
  }

  function flagText(row: Row): string {
    const flags: string[] = [];
    if (row.fault) flags.push("FAULT");
    if (row.error) flags.push("ERROR");
    if (row.throttle) flags.push("THROTTLE");
    return flags.join(" ");
  }

  function spanTags(row: Row): string[] {
    const tags: string[] = [];
    if (row.namespace) tags.push(row.namespace);
    if (row.aws_operation) tags.push(row.aws_operation);
    if (row.http_status != null) tags.push(`http ${row.http_status}`);
    return tags;
  }

  function leaf(path: string): string {
    return path.split("/").pop() ?? path;
  }

  function submitPaste() {
    const id = pasteId.trim();
    if (!isTraceId(id)) return;
    onOpenTrace(id);
  }

  $effect(() => {
    if (trace) selPath = null;
  });

  $effect(() => {
    const want = focusSpanName?.trim();
    if (!want || !trace) return;
    const hit = rows.find((r) => r.name === want);
    if (hit) selPath = hit.path;
  });
</script>

<aside class="trace">
  <div class="paste">
    <label class="label" for="trace-paste">Open trace</label>
    <div class="paste-row">
      <input
        id="trace-paste"
        class="input mono"
        bind:value={pasteId}
        placeholder="1-aaaaaaaa-bbbbbbbbbbbbbbbbbbbbbbbb"
        onkeydown={(e) => e.key === "Enter" && submitPaste()}
      />
      <button type="button" class="btn btn-primary btn-sm" disabled={!pasteOk || loading} onclick={submitPaste}>
        {loading ? "…" : "Open"}
      </button>
    </div>
    <button
      type="button"
      class="btn btn-ghost btn-sm browse"
      disabled={summariesBusy}
      onclick={loadSummaries}
    >
      {summariesBusy ? "Loading…" : "Browse recent traces"}
    </button>
    {#if summariesErr}<div class="dim tip">{summariesErr}</div>{/if}
    {#if summaries.length > 0}
      <div class="summaries">
        {#each summaries as s (s.id)}
          <button type="button" class="sum-hit mono" onclick={() => onOpenTrace(s.id)}>
            <span class:fault={s.has_fault || s.has_error}>{s.id.slice(0, 22)}…</span>
            <span class="dim"
              >{s.duration_secs != null ? `${(s.duration_secs * 1000).toFixed(0)}ms` : ""}</span
            >
          </button>
        {/each}
      </div>
    {/if}
  </div>

  {#if error}
    <div class="err" role="alert">
      <strong>{error}</strong>
      {#if errorHint}<div class="err-hint">{errorHint}</div>{/if}
      {#if errorCommand}
        <div class="err-cmd">
          <code class="mono">{errorCommand}</code>
          <button type="button" class="btn btn-ghost btn-sm" onclick={copyCmd}
            >{cmdCopied ? "Copied" : "Copy"}</button
          >
        </div>
      {/if}
    </div>
  {/if}

  {#if loading && !trace}
    <div class="empty">Loading trace…</div>
  {:else if !trace}
    <div class="empty">
      <p>No waterfall yet.</p>
      <p class="dim tip">
        Click a trace or request pill, paste an ID, or browse recent traces above.
      </p>
    </div>
  {:else}
    <div class="head">
      <span class="label">Trace</span>
      <span class="badge badge-dim mono">{spanCount} spans</span>
      <span class="spacer"></span>
      <button type="button" class="btn btn-ghost btn-sm" onclick={copyId}
        >{idCopied ? "Copied" : "Copy ID"}</button
      >
      {#if region}
        <button type="button" class="btn btn-ghost btn-sm" onclick={openInConsole}>Console</button>
      {/if}
    </div>
    <div class="mono tid" title={trace.id}>{trace.id}</div>
    <div class="meta mono dim">
      {#if trace.duration_secs != null}
        Duration: <strong>{(trace.duration_secs * 1000).toFixed(0)} ms</strong>
      {/if}
    </div>
    <div class="scale mono dim">
      <span>0ms</span><span>{Math.round(span / 2)}ms</span><span>{span}ms</span>
    </div>
    {#each rows as row (row.path)}
      {@const left = ((row.start_ms - minStart) / span) * 100}
      {@const width = Math.max(((row.end_ms - row.start_ms) / span) * 100, 1.5)}
      <button
        type="button"
        class="seg"
        class:sel={selPath === row.path}
        class:sub={row.kind === "subsegment"}
        class:failed={rowFailed(row)}
        style={`padding-left: ${row.depth * 12}px`}
        onclick={() => (selPath = row.path)}
      >
        <div class="seg-top">
          <span class="seg-label">
            {#if row.hasChildren}
              <span
                class="chev"
                role="button"
                tabindex="0"
                aria-label={collapsed[row.path] ? "Expand" : "Collapse"}
                onclick={(e) => {
                  e.stopPropagation();
                  toggleCollapse(row.path);
                }}
                onkeydown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.stopPropagation();
                    toggleCollapse(row.path);
                  }
                }}
                >{collapsed[row.path] ? "▸" : "▾"}</span
              >
            {:else}
              <span class="chev-spacer"></span>
            {/if}
            <span class="mono">{row.name || row.path}</span>
            {#if flagText(row)}<span class="flag mono">{flagText(row)}</span>{/if}
            {#each spanTags(row) as tag}
              <span class="tag mono dim">{tag}</span>
            {/each}
          </span>
          <span class="mono dim">{row.end_ms - row.start_ms} ms</span>
        </div>
        <div class="bar">
          <i style={`margin-left: ${left}%; width: ${width}%; background: ${barColor(row)}`}
          ></i>
        </div>
      </button>
    {:else}
      <p class="dim empty-seg">Trace loaded but has no segments.</p>
    {/each}
    {#if sel}
      <div class="detail card">
        <div class="label">{sel.kind === "segment" ? "Segment" : "Subsegment"}</div>
        <div class="kv mono"><span>Name</span><span>{sel.name}</span></div>
        <div class="kv mono"><span>Duration</span><span>{sel.end_ms - sel.start_ms} ms</span></div>
        {#if sel.origin}<div class="kv mono"><span>Origin</span><span>{sel.origin}</span></div>{/if}
        {#if sel.namespace}<div class="kv mono"><span>Namespace</span><span>{sel.namespace}</span></div>{/if}
        {#if sel.aws_operation}<div class="kv mono"><span>Operation</span><span>{sel.aws_operation}</span></div>{/if}
        {#if sel.http_status != null}
          <div class="kv mono"><span>HTTP status</span><span>{sel.http_status}</span></div>
        {/if}
        {#if flagText(sel)}
          <div class="kv mono"><span>Status</span><span class="flag">{flagText(sel)}</span></div>
        {/if}
        {#if sel.cause}
          <div class="cause mono">{sel.cause}</div>
        {/if}
        <button
          type="button"
          class="btn btn-primary"
          style="width: 100%; justify-content: center; margin-top: 10px"
          onclick={() => onViewLogs(sel.start_ms - 300_000, sel.end_ms + 300_000, matchedNode?.construct_path)}
        >
          {matchedNode ? `View logs → ${leaf(matchedNode.construct_path)}` : "View logs ±5 min"}
        </button>
      </div>
    {/if}
  {/if}
</aside>

<style>
  .trace {
    height: 100%;
    overflow-y: auto;
    padding: 12px;
    background: var(--bg1);
  }
  .paste {
    margin-bottom: 12px;
  }
  .paste-row {
    display: flex;
    gap: 6px;
    margin-top: 6px;
  }
  .paste-row .input {
    flex: 1;
    min-width: 0;
    font-size: 11px;
  }
  .browse {
    margin-top: 8px;
    width: 100%;
    justify-content: center;
  }
  .summaries {
    margin-top: 8px;
    max-height: 140px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .sum-hit {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border);
    color: var(--text);
    font-size: 11px;
    padding: 6px 8px;
    cursor: pointer;
  }
  .sum-hit:last-child {
    border-bottom: none;
  }
  .sum-hit:hover {
    background: var(--hover);
  }
  .sum-hit .fault {
    color: var(--red-bright, #e07070);
  }
  .err {
    font-size: 12px;
    color: var(--text);
    background: rgba(224, 112, 112, 0.1);
    border: 1px solid rgba(224, 112, 112, 0.35);
    border-radius: 6px;
    padding: 8px 10px;
    margin-bottom: 10px;
    word-break: break-word;
  }
  .err strong {
    color: var(--red-bright);
  }
  .err-hint {
    margin-top: 4px;
    color: var(--text-dim);
    line-height: 1.4;
  }
  .err-cmd {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 8px;
    flex-wrap: wrap;
  }
  .empty {
    padding: 16px 4px;
    color: var(--text-dim);
    font-size: 13px;
  }
  .tip {
    margin-top: 8px;
    font-size: 11px;
    line-height: 1.45;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }
  .spacer {
    flex: 1;
  }
  .tid {
    font-size: 11px;
    word-break: break-all;
    margin-bottom: 6px;
  }
  .meta {
    font-size: 11px;
    margin-bottom: 8px;
  }
  .scale {
    display: flex;
    justify-content: space-between;
    font-size: 10px;
    margin-bottom: 6px;
  }
  .seg {
    display: block;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: inherit;
    padding: 6px 0;
    cursor: pointer;
    border-radius: 4px;
  }
  .seg:hover {
    background: var(--hover);
  }
  .seg.sub {
    opacity: 0.92;
  }
  .seg.failed .bar i {
    box-shadow: 0 0 6px rgba(224, 112, 112, 0.55);
  }
  .seg.failed .seg-top .mono {
    color: var(--red-bright, #e07070);
  }
  .seg.sel .bar {
    outline: 1px solid var(--amber);
  }
  .seg-top {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 8px;
    font-size: 11px;
    margin-bottom: 3px;
  }
  .seg-label {
    display: flex;
    align-items: baseline;
    gap: 5px;
    min-width: 0;
    flex-wrap: wrap;
  }
  .chev {
    cursor: pointer;
    color: var(--text-dim);
    font-size: 10px;
    width: 10px;
    display: inline-block;
  }
  .chev:hover {
    color: var(--text);
  }
  .chev-spacer {
    display: inline-block;
    width: 10px;
  }
  .flag {
    color: var(--red-bright, #e07070);
    font-size: 9px;
    letter-spacing: 0.4px;
  }
  .tag {
    font-size: 9px;
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0 3px;
  }
  .bar {
    position: relative;
    height: 8px;
    background: var(--bg2);
    border-radius: 2px;
    overflow: hidden;
  }
  .bar i {
    display: block;
    height: 100%;
    border-radius: 2px;
  }
  .detail {
    margin-top: 12px;
    padding: 10px;
  }
  .kv {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 11px;
    margin-top: 4px;
  }
  .cause {
    margin-top: 8px;
    font-size: 10px;
    color: var(--red-bright, #e07070);
    word-break: break-word;
  }
  .empty-seg {
    font-size: 12px;
    margin-top: 8px;
  }
</style>
