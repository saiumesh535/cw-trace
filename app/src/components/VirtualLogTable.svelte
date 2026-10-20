<script lang="ts">
  import type { LogLine } from "../lib/types";
  import { requestIdsForLine, severityOf, traceIdsForLine } from "../lib/trace";

  let {
    lines,
    selectedMs = null,
    gotoMs = null,
    autoScroll = false,
    onSelect,
    onOpenTrace,
    onOpenRequestId,
    onUserScrollUp,
  }: {
    lines: LogLine[];
    selectedMs?: number | null;
    /** Scroll to the first line with this timestamp when it changes. */
    gotoMs?: number | null;
    /** Keep the newest line in view as lines arrive (tail). */
    autoScroll?: boolean;
    onSelect?: (line: LogLine) => void;
    onOpenTrace?: (id: string) => void;
    onOpenRequestId?: (id: string, aroundMs: number) => void;
    /** The user scrolled away from the bottom while autoScroll is on. */
    onUserScrollUp?: () => void;
  } = $props();

  const ROW_H = 64;
  let viewport: HTMLDivElement | null = $state(null);
  let scrollTop = $state(0);
  let viewH = $state(600);
  let expandedIdx = $state<number | null>(null);
  let copiedIdx = $state<number | null>(null);

  function pad(n: number, w = 2): string {
    return String(n).padStart(w, "0");
  }

  /** Wall-clock time in the *local* timezone (CloudWatch ships epoch millis). */
  function fmtLocal(ms: number): string {
    const d = new Date(ms);
    return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
  }

  const start = $derived(Math.max(0, Math.floor(scrollTop / ROW_H) - 4));
  const count = $derived(Math.ceil(viewH / ROW_H) + 8);
  const visible = $derived(lines.slice(start, start + count));
  const expanded = $derived(
    expandedIdx != null && expandedIdx >= start && expandedIdx < start + count
      ? lines[expandedIdx]
      : null,
  );

  let lastGoto: number | null = null;
  $effect(() => {
    if (gotoMs != null && gotoMs !== lastGoto) {
      lastGoto = gotoMs;
      const idx = lines.findIndex((l) => l.timestampMs === gotoMs);
      if (idx >= 0 && viewport) {
        viewport.scrollTo({ top: Math.max(0, idx * ROW_H - 80) });
      }
    }
  });

  // Auto-scroll to the newest line while tailing; paused when the user
  // scrolls up (they're reading history).
  $effect(() => {
    const n = lines.length;
    if (autoScroll && n > 0 && viewport) {
      queueMicrotask(() => {
        if (autoScroll && viewport) {
          viewport.scrollTop = viewport.scrollHeight;
          scrollTop = viewport.scrollTop;
        }
      });
    }
  });

  function handleScroll() {
    const el = viewport;
    if (!el) return;
    scrollTop = el.scrollTop;
    viewH = el.clientHeight;
    if (autoScroll && el.scrollHeight - el.scrollTop - el.clientHeight > 48) {
      onUserScrollUp?.();
    }
  }

  /** Parse a log message as JSON when it looks like one (Lambda JSON logs). */
  function parseJson(msg: string): unknown | null {
    const t = msg.trim();
    if (!t.startsWith("{") && !t.startsWith("[")) return null;
    try {
      return JSON.parse(t);
    } catch {
      return null;
    }
  }

  function prettyMessage(msg: string): string {
    const parsed = parseJson(msg);
    return parsed != null ? JSON.stringify(parsed, null, 2) : msg;
  }

  function isJson(msg: string): boolean {
    return parseJson(msg) != null;
  }

  async function copyLine(e: MouseEvent, line: LogLine, idx: number) {
    e.stopPropagation();
    e.preventDefault();
    try {
      await navigator.clipboard.writeText(line.message);
      copiedIdx = idx;
      setTimeout(() => {
        if (copiedIdx === idx) copiedIdx = null;
      }, 1500);
    } catch {
      // ignore
    }
  }

  function toggleExpand(e: MouseEvent, idx: number) {
    e.stopPropagation();
    e.preventDefault();
    expandedIdx = expandedIdx === idx ? null : idx;
  }

  function openTrace(e: MouseEvent, id: string) {
    e.stopPropagation();
    e.preventDefault();
    onOpenTrace?.(id);
  }

  function openRequest(e: MouseEvent, id: string, aroundMs: number) {
    e.stopPropagation();
    e.preventDefault();
    onOpenRequestId?.(id, aroundMs);
  }
</script>

<div
  class="viewport"
  bind:this={viewport}
  onscroll={handleScroll}
>
  <div class="spacer" style={`height: ${lines.length * ROW_H}px`}>
    {#each visible as l, i (l.timestampMs + "-" + (start + i))}
      {@const idx = start + i}
      {@const lvl = severityOf(l.message)}
      {@const ids = traceIdsForLine(l.message, l.traceIds)}
      {@const rids = requestIdsForLine(l.message, l.lambdaRequestIds)}
      <!-- div not button: nested interactive pills must receive clicks -->
      <div
        class="row"
        class:sel={selectedMs === l.timestampMs}
        class:err-row={lvl === "ERROR"}
        class:warn-row={lvl === "WARN"}
        style={`top: ${idx * ROW_H}px; height: ${ROW_H}px`}
        role="button"
        tabindex="0"
        onclick={() => onSelect?.(l)}
        onkeydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            onSelect?.(l);
          }
        }}
      >
        <span class="ts mono" title={new Date(l.timestampMs).toISOString()}>{fmtLocal(l.timestampMs)}</span>
        <span class="body">
          <span class="line1"
            ><span
              class="badge {lvl === 'ERROR'
                ? 'badge-err'
                : lvl === 'WARN'
                  ? 'badge-warn'
                  : lvl === 'INFO'
                    ? 'badge-dim'
                    : 'badge-info'}">{lvl}</span
            >
            {l.message}</span
          >
          <span class="line2 mono dim">{l.logGroup}/{l.logStream}</span>
        </span>
        <span class="pills">
          {#if ids.length > 0 || rids.length > 0}
            {#each ids.slice(0, 2) as t (t)}
              <button
                type="button"
                class="trace-pill mono"
                title={`Open X-Ray trace ${t}`}
                onclick={(e) => openTrace(e, t)}
              >
                {t.slice(0, 18)}…
              </button>
            {/each}
            {#each rids.slice(0, 1) as r (r)}
              <button
                type="button"
                class="req-pill mono"
                title={`Find X-Ray trace for RequestId ${r}`}
                onclick={(e) => openRequest(e, r, l.timestampMs)}
              >
                req {r.slice(0, 8)}…
              </button>
            {/each}
          {/if}
          <button
            type="button"
            class="mini-pill mono"
            class:json={isJson(l.message)}
            title={expandedIdx === idx ? "Collapse message" : "Expand message"}
            onclick={(e) => toggleExpand(e, idx)}
          >
            {isJson(l.message) ? "{ }" : "≡"}
          </button>
          <button
            type="button"
            class="mini-pill mono"
            title="Copy message"
            onclick={(e) => copyLine(e, l, idx)}
          >
            {copiedIdx === idx ? "✓" : "copy"}
          </button>
        </span>
      </div>
    {/each}
    {#if expanded}
      {@const eidx = expandedIdx as number}
      <div class="expand" style={`top: ${(eidx + 1) * ROW_H}px`}>
        <div class="expand-head">
          <span class="mono dim expand-src">{expanded.logGroup}/{expanded.logStream}</span>
          <span class="expander"></span>
          {#if isJson(expanded.message)}
            <span class="badge badge-dim mono">json</span>
          {/if}
          <button
            type="button"
            class="mini-pill mono"
            onclick={(e) => copyLine(e, expanded, eidx)}
          >
            {copiedIdx === eidx ? "✓ copied" : "copy"}
          </button>
          <button
            type="button"
            class="mini-pill mono"
            onclick={(e) => {
              e.stopPropagation();
              expandedIdx = null;
            }}
          >
            close
          </button>
        </div>
        <pre class="mono expand-body">{prettyMessage(expanded.message)}</pre>
      </div>
    {/if}
  </div>
  {#if lines.length === 0}
    <div class="empty">No events in this range — widen it or check the filter.</div>
  {/if}
</div>

<style>
  .viewport {
    position: relative;
    height: 100%;
    overflow-y: auto;
    background: var(--bg0);
  }
  .spacer {
    position: relative;
  }
  .row {
    position: absolute;
    left: 0;
    right: 0;
    display: flex;
    align-items: flex-start;
    gap: 10px;
    background: none;
    border: none;
    border-bottom: 1px solid var(--border);
    color: var(--text);
    text-align: left;
    padding: 8px 12px;
    font-size: 12px;
    overflow: hidden;
    cursor: pointer;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.sel {
    background: rgba(249, 115, 22, 0.07);
    border-left: 2px solid var(--amber);
  }
  .row.err-row {
    border-left: 2px solid var(--red, #e07070);
    background: rgba(224, 112, 112, 0.05);
  }
  .row.warn-row {
    border-left: 2px solid var(--amber);
    background: rgba(249, 115, 22, 0.04);
  }
  .ts {
    flex: none;
    color: var(--text-faint);
    font-size: 11px;
    padding-top: 2px;
  }
  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .line1 {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .line2 {
    font-size: 10px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pills {
    flex: none;
    display: flex;
    align-items: flex-start;
    gap: 6px;
    padding-top: 2px;
  }
  .req-pill {
    font-size: 10px;
    color: var(--sage, #6b8f71);
    background: rgba(107, 143, 113, 0.14);
    border: 1px solid rgba(107, 143, 113, 0.4);
    border-radius: 4px;
    padding: 2px 7px;
    cursor: pointer;
    font-family: inherit;
  }
  .req-pill:hover {
    background: rgba(107, 143, 113, 0.28);
  }
  .trace-pill {
    font-size: 10px;
    color: var(--amber);
    background: rgba(249, 115, 22, 0.14);
    border: 1px solid rgba(249, 115, 22, 0.4);
    border-radius: 4px;
    padding: 2px 7px;
    cursor: pointer;
    font-family: var(--font-mono);
  }
  .trace-pill:hover {
    background: rgba(249, 115, 22, 0.28);
  }
  .mini-pill {
    font-size: 10px;
    color: var(--text-dim);
    background: none;
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 2px 7px;
    cursor: pointer;
    font-family: var(--font-mono);
    opacity: 0;
  }
  .row:hover .mini-pill,
  .mini-pill.json,
  .mini-pill:focus-visible {
    opacity: 1;
  }
  .mini-pill:hover {
    color: var(--text);
    border-color: var(--text-faint);
  }
  .mini-pill.json {
    color: var(--amber);
    border-color: rgba(249, 115, 22, 0.4);
  }
  .expand {
    position: absolute;
    left: 8px;
    right: 8px;
    z-index: 5;
    background: var(--bg1, #14161c);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.45);
    padding: 8px 10px;
    cursor: default;
  }
  .expand-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }
  .expand-head .mini-pill {
    opacity: 1;
  }
  .expand-src {
    font-size: 10px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .expander {
    flex: 1;
  }
  .expand-body {
    margin: 0;
    font-size: 11px;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 320px;
    overflow-y: auto;
    color: var(--text);
  }
  .empty {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-faint);
    font-size: 12px;
  }
</style>
