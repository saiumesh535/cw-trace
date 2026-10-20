<script lang="ts">
  import { store } from "../lib/store.svelte";
  import type { ResourceNode } from "../lib/types";

  let { graphNodes = [], onOpenTrace }: {
    graphNodes?: ResourceNode[];
    onOpenTrace?: (id: string) => void;
  } = $props();

  let q = $state("");
  let inputEl: HTMLInputElement | null = $state(null);

  const stageHits = $derived(
    q
      ? store.stages
          .filter((s) =>
            `${s.name} ${store.projectName(s.projectId)}`.toLowerCase().includes(q.toLowerCase()),
          )
          .slice(0, 5)
      : store.pinnedStages().slice(0, 5),
  );
  const constructHits = $derived(
    q
      ? graphNodes
          .filter((n) => n.construct_path.toLowerCase().includes(q.toLowerCase()))
          .slice(0, 6)
      : [],
  );
  const looksLikeTraceId = $derived(/^1-[0-9a-f]{4,}/i.test(q.trim()));

  $effect(() => {
    if (store.paletteOpen) {
      q = "";
      queueMicrotask(() => inputEl?.focus());
    }
  });

  function close() {
    store.paletteOpen = false;
  }
  function openStage(id: string) {
    close();
    store.openStage(id);
  }
</script>

{#if store.paletteOpen}
  <div
    class="overlay"
    role="button"
    tabindex="-1"
    aria-label="Close palette"
    onclick={close}
    onkeydown={(e) => e.key === "Escape" && close()}
  >
    <div
      class="palette card"
      role="dialog"
      tabindex="-1"
      aria-modal="true"
      aria-label="Command palette"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
    >
      <div class="row">
        <span class="material-symbols-outlined">search</span>
        <input
          bind:this={inputEl}
          bind:value={q}
          placeholder="Jump to a stage, construct, or paste a trace ID…"
          onkeydown={(e) => e.key === "Escape" && close()}
        />
        <kbd>esc</kbd>
      </div>
      <div class="groups">
        <div class="label">Stages</div>
        {#each stageHits as s (s.id)}
          <button class="hit" onclick={() => openStage(s.id)}>
            <span class="dot {s.lastError ? 'dot-err' : 'dot-ok'}"></span>
            <span class="mono">{s.name} — {store.projectName(s.projectId)}</span>
            <span class="dim mono" style="font-size: 11px">{s.region ?? ""}</span>
          </button>
        {:else}
          <div class="dim" style="padding: 4px 10px; font-size: 12px">No stages yet.</div>
        {/each}
        {#if constructHits.length > 0}
          <div class="label" style="margin-top: 8px">Constructs</div>
          {#each constructHits as n (n.construct_path)}
            <button
              class="hit"
              onclick={() => {
                close();
                if (store.activeStageId) {
                  store.pendingConstruct = n.construct_path;
                  store.view = "debugger";
                }
              }}
            >
              <span class="material-symbols-outlined">account_tree</span>
              <span class="mono">{n.construct_path}</span>
            </button>
          {/each}
        {/if}
        {#if looksLikeTraceId}
          <div class="label" style="margin-top: 8px">Trace</div>
          <button
            class="hit"
            onclick={() => {
              const id = q.trim();
              close();
              store.pendingTraceId = id;
              if (store.activeStageId) store.view = "debugger";
              onOpenTrace?.(id);
            }}
          >
            <span class="material-symbols-outlined">waterfall_chart</span>
            <span class="mono">Open trace {q.trim().slice(0, 28)}…</span>
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    z-index: 60;
    display: flex;
    justify-content: center;
    padding-top: 12vh;
  }
  .palette {
    width: 560px;
    max-height: 60vh;
    overflow: hidden auto;
    background: var(--bg2);
    box-shadow: 0 8px 32px -4px rgba(0, 0, 0, 0.7), 0 0 0 1px rgba(255, 255, 255, 0.1);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
  }
  .row input {
    flex: 1;
    background: none;
    border: none;
    outline: none;
    color: var(--text);
    font-size: 13px;
  }
  .groups {
    padding: 8px;
  }
  .label {
    padding: 2px 10px 4px;
  }
  .hit {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: var(--text);
    font-size: 12px;
    padding: 7px 10px;
    border-radius: 6px;
  }
  .hit:hover {
    background: var(--hover);
  }
  .hit .material-symbols-outlined {
    font-size: 16px;
    color: var(--text-dim);
  }
</style>
