<script lang="ts">
  import type { NodeHeat, ResourceEdge, ResourceNode } from "../lib/types";
  import { mapColumn } from "../lib/trace";
  import { onDestroy, onMount } from "svelte";

  let {
    nodes = [],
    edges = [],
    selected = null,
    heat = {},
    tracePath = [],
    stackFilter = null,
    onSelect,
    onSelectSpan,
  }: {
    nodes: ResourceNode[];
    edges: ResourceEdge[];
    selected?: string | null;
    heat?: Record<string, NodeHeat>;
    /** Ordered construct paths from the open trace. */
    tracePath?: string[];
    /** Limit to one stack when set. */
    stackFilter?: string | null;
    onSelect: (constructPath: string) => void;
    onSelectSpan?: (spanName: string) => void;
  } = $props();

  const COL_W = 118;
  const ROW_H = 44;
  const PAD_X = 16;
  const PAD_Y = 28;
  const NODE_W = 100;
  const NODE_H = 32;
  const FOCUS_LIMIT = 60;
  const PULSE_MS = 600;
  const ERR_HALFLIFE_MS = 8000;

  let nowMs = $state(Date.now());
  let raf = 0;
  let infoPath = $state<string | null>(null);

  onMount(() => {
    const tick = () => {
      nowMs = Date.now();
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  });

  onDestroy(() => {
    if (raf) cancelAnimationFrame(raf);
  });

  function leaf(path: string): string {
    return path.split("/").pop() ?? path;
  }

  function stackOf(path: string): string {
    return path.split("/")[0] ?? path;
  }

  const mapNodes = $derived.by(() => {
    const covered = new Set(
      nodes.filter((n) => n.kind !== "log-group" && n.log_group).map((n) => n.log_group!),
    );
    return nodes.filter((n) => {
      if (n.kind === "log-group" && n.log_group && covered.has(n.log_group)) return false;
      if (mapColumn(n.kind) == null && !n.log_group) return false;
      if (stackFilter && stackOf(n.construct_path) !== stackFilter) return false;
      return mapColumn(n.kind) != null || !!n.log_group;
    });
  });

  const layout = $derived.by(() => {
    const cols: Record<"in" | "compute" | "data", ResourceNode[]> = {
      in: [],
      compute: [],
      data: [],
    };
    for (const n of mapNodes) {
      const col = mapColumn(n.kind) ?? (n.log_group ? "compute" : null);
      if (col) cols[col].push(n);
    }
    for (const k of ["in", "compute", "data"] as const) {
      cols[k].sort((a, b) => a.construct_path.localeCompare(b.construct_path));
    }

    let visible = [...cols.in, ...cols.compute, ...cols.data];
    if (visible.length > FOCUS_LIMIT) {
      const keep = new Set<string>();
      if (selected) {
        keep.add(selected);
        for (const e of edges) {
          if (e.from === selected || e.to === selected) {
            keep.add(e.from);
            keep.add(e.to);
          }
        }
        // 2-hop neighbors
        for (const e of edges) {
          if (keep.has(e.from) || keep.has(e.to)) {
            keep.add(e.from);
            keep.add(e.to);
          }
        }
      }
      for (const path of Object.keys(heat)) keep.add(path);
      for (const p of tracePath) keep.add(p);
      if (keep.size === 0) {
        visible = visible.slice(0, FOCUS_LIMIT);
      } else {
        visible = visible.filter((n) => keep.has(n.construct_path));
      }
      // Rebuild columns from focused set.
      cols.in = [];
      cols.compute = [];
      cols.data = [];
      for (const n of visible) {
        const col = mapColumn(n.kind) ?? (n.log_group ? "compute" : null);
        if (col) cols[col].push(n);
      }
    }

    const positions = new Map<string, { x: number; y: number; col: string }>();
    const colIndex = { in: 0, compute: 1, data: 2 } as const;
    let maxRows = 0;
    for (const [name, list] of Object.entries(cols) as [keyof typeof cols, ResourceNode[]][]) {
      list.forEach((n, i) => {
        positions.set(n.construct_path, {
          x: PAD_X + colIndex[name] * COL_W,
          y: PAD_Y + i * ROW_H,
          col: name,
        });
      });
      maxRows = Math.max(maxRows, list.length);
    }

    const width = PAD_X * 2 + COL_W * 3 - (COL_W - NODE_W);
    const height = Math.max(PAD_Y + maxRows * ROW_H + 8, 120);
    return { cols, positions, width, height, count: visible.length };
  });

  const drawnEdges = $derived.by(() => {
    const paths = new Set(layout.positions.keys());
    return edges.filter((e) => paths.has(e.from) && paths.has(e.to));
  });

  const pathEdgeKeys = $derived.by(() => {
    const keys = new Set<string>();
    for (let i = 0; i < tracePath.length - 1; i++) {
      keys.add(`${tracePath[i]}→${tracePath[i + 1]}`);
      keys.add(`${tracePath[i + 1]}→${tracePath[i]}`);
    }
    return keys;
  });

  const pathSegments = $derived.by(() => {
    const segs: { x1: number; y1: number; x2: number; y2: number; key: string }[] = [];
    for (let i = 0; i < tracePath.length - 1; i++) {
      const a = layout.positions.get(tracePath[i]);
      const b = layout.positions.get(tracePath[i + 1]);
      if (!a || !b) continue;
      segs.push({
        x1: a.x + NODE_W,
        y1: a.y + NODE_H / 2,
        x2: b.x,
        y2: b.y + NODE_H / 2,
        key: `${tracePath[i]}→${tracePath[i + 1]}`,
      });
    }
    return segs;
  });

  function heatOpacity(path: string): number {
    const h = heat[path];
    if (!h) return 0;
    const age = nowMs - h.lastPulseMs;
    if (age < PULSE_MS) return 0.85;
    if (h.severity === "ERROR" || h.severity === "WARN") {
      // Half-life decay for error halo.
      return Math.max(0, 0.55 * Math.pow(0.5, age / ERR_HALFLIFE_MS));
    }
    return Math.max(0, 0.4 * (1 - age / 4000));
  }

  function heatColor(path: string): string {
    const h = heat[path];
    if (!h) return "var(--amber)";
    if (h.severity === "ERROR") return "var(--red)";
    if (h.severity === "WARN") return "var(--copper)";
    return "var(--amber)";
  }

  function tip(path: string): string {
    const n = nodes.find((x) => x.construct_path === path);
    const h = heat[path];
    const parts = [n?.log_group ?? n?.resource_name ?? n?.cfn_type ?? path];
    if (h && h.errCount > 0) {
      const rate = `${h.errCount} err / 10s`;
      parts.push(rate);
      if (h.lastError) parts.push(h.lastError.slice(0, 80));
    }
    return parts.filter(Boolean).join(" · ");
  }

  const infoNode = $derived(infoPath ? nodes.find((n) => n.construct_path === infoPath) : null);
  const infoRefs = $derived.by(() => {
    if (!infoPath) return [] as string[];
    return edges.filter((e) => e.to === infoPath).map((e) => leaf(e.from));
  });

  function clickNode(n: ResourceNode) {
    if (n.log_group) {
      infoPath = null;
      onSelect(n.construct_path);
    } else {
      infoPath = infoPath === n.construct_path ? null : n.construct_path;
    }
  }

  function clickPathEdge(from: string, to: string) {
    const dest = nodes.find((n) => n.construct_path === to);
    const name = dest?.function_name ?? dest?.resource_name ?? leaf(to);
    onSelectSpan?.(name);
    if (dest?.log_group) onSelect(to);
    else infoPath = to;
  }
</script>

<div class="cmap">
  {#if layout.count === 0}
    <p class="dim empty">No map nodes for this stack.</p>
  {:else}
    {#if drawnEdges.length === 0 && pathSegments.length === 0}
      <p class="dim note">No dependency edges in this synth.</p>
    {/if}
    <svg
      class="svg"
      width={layout.width}
      height={layout.height}
      viewBox={`0 0 ${layout.width} ${layout.height}`}
      role="img"
      aria-label="Construct map"
    >
      <text x={PAD_X} y={14} class="col-label">in</text>
      <text x={PAD_X + COL_W} y={14} class="col-label">compute</text>
      <text x={PAD_X + COL_W * 2} y={14} class="col-label">data</text>

      {#each drawnEdges as e (`${e.from}->${e.to}:${e.via}`)}
        {@const a = layout.positions.get(e.from)}
        {@const b = layout.positions.get(e.to)}
        {#if a && b}
          {@const lit = pathEdgeKeys.has(`${e.from}→${e.to}`)}
          <line
            x1={a.x + NODE_W}
            y1={a.y + NODE_H / 2}
            x2={b.x}
            y2={b.y + NODE_H / 2}
            class="edge"
            class:lit
          />
        {/if}
      {/each}

      {#each pathSegments as seg (seg.key)}
        <g
          class="path-hit"
          role="button"
          tabindex="0"
          aria-label="Select span on path"
          onclick={() => {
            const [from, to] = seg.key.split("→");
            clickPathEdge(from, to);
          }}
          onkeydown={(e) => {
            if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              const [from, to] = seg.key.split("→");
              clickPathEdge(from, to);
            }
          }}
        >
          <line x1={seg.x1} y1={seg.y1} x2={seg.x2} y2={seg.y2} class="edge path" />
          <line
            x1={seg.x1}
            y1={seg.y1}
            x2={seg.x2}
            y2={seg.y2}
            class="edge path-wide"
          />
          <circle r="3" class="path-dot">
            <animateMotion
              dur="1.6s"
              repeatCount="indefinite"
              path={`M ${seg.x1} ${seg.y1} L ${seg.x2} ${seg.y2}`}
            />
          </circle>
        </g>
      {/each}

      {#each [...layout.positions.entries()] as [path, pos] (path)}
        {@const n = nodes.find((x) => x.construct_path === path)}
        {#if n}
          {@const op = heatOpacity(path)}
          {@const isSel = selected === path}
          <g
            class="node"
            class:sel={isSel}
            class:nolog={!n.log_group}
            transform={`translate(${pos.x},${pos.y})`}
            onclick={() => clickNode(n)}
            onkeydown={(e) => e.key === "Enter" && clickNode(n)}
            role="button"
            tabindex="0"
          >
            <title>{tip(path)}</title>
            {#if op > 0.02}
              <rect
                class="halo"
                x={-3}
                y={-3}
                width={NODE_W + 6}
                height={NODE_H + 6}
                rx="8"
                style={`opacity:${op}; stroke:${heatColor(path)}`}
              />
            {/if}
            <rect class="card" width={NODE_W} height={NODE_H} rx="6" />
            <text class="label" x={8} y={20}>{leaf(path)}</text>
          </g>
        {/if}
      {/each}
    </svg>

    {#if infoNode}
      <div class="card info">
        <div class="info-head">
          <strong class="mono">{leaf(infoNode.construct_path)}</strong>
          <button type="button" class="btn btn-ghost btn-sm" onclick={() => (infoPath = null)}
            >Close</button
          >
        </div>
        <div class="kv mono"><span>Type</span><span>{infoNode.cfn_type}</span></div>
        {#if infoNode.logical_id}
          <div class="kv mono"><span>Logical ID</span><span>{infoNode.logical_id}</span></div>
        {/if}
        {#if infoNode.resource_name}
          <div class="kv mono"><span>Name</span><span>{infoNode.resource_name}</span></div>
        {/if}
        {#if infoRefs.length > 0}
          <div class="kv mono"><span>Referenced by</span><span>{infoRefs.join(", ")}</span></div>
        {/if}
        <p class="dim tip">No log group — select a compute node to tail.</p>
      </div>
    {/if}
  {/if}
</div>

<style>
  .cmap {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
    flex: 1;
    overflow: auto;
    padding: 4px 6px 10px;
  }
  .empty,
  .note {
    margin: 8px 4px;
    font-size: 11px;
  }
  .svg {
    display: block;
    max-width: 100%;
  }
  .col-label {
    fill: var(--text-faint);
    font-size: 10px;
    font-family: var(--font-mono);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .edge {
    stroke: var(--border-strong);
    stroke-width: 1;
    fill: none;
    opacity: 0.55;
  }
  .edge.lit,
  .edge.path {
    stroke: var(--amber);
    stroke-width: 2;
    opacity: 0.95;
  }
  .edge.path-wide {
    stroke: transparent;
    stroke-width: 12;
    opacity: 1;
  }
  .path-hit {
    cursor: pointer;
  }
  .path-dot {
    fill: var(--amber);
  }
  .node {
    cursor: pointer;
  }
  .node .card {
    fill: var(--bg2);
    stroke: var(--border-strong);
    stroke-width: 1;
  }
  .node.sel .card {
    stroke: var(--amber);
    stroke-width: 1.5;
  }
  .node.nolog .card {
    fill: var(--bg1);
    stroke-dasharray: 3 2;
  }
  .node .label {
    fill: var(--text);
    font-size: 10px;
    font-family: var(--font-mono);
    pointer-events: none;
  }
  .halo {
    fill: none;
    stroke-width: 2;
    pointer-events: none;
  }
  .info {
    margin: 4px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--bg1);
  }
  .info-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 6px;
  }
  .kv {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 11px;
    margin: 2px 0;
  }
  .kv span:first-child {
    color: var(--text-faint);
  }
  .tip {
    margin: 6px 0 0;
    font-size: 11px;
  }
</style>
