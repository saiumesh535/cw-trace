<script lang="ts">
  import { confirm } from "@tauri-apps/plugin-dialog";
  import { store, timeAgo } from "../lib/store.svelte";
  import type { Project, Stage } from "../lib/types";

  let {
    onAddProject,
    onAddStage,
    onEditStage,
    onToast,
  }: {
    onAddProject: () => void;
    onAddStage: (project: Project) => void;
    onEditStage: (stage: Stage) => void;
    onToast: (msg: string) => void;
  } = $props();

  let search = $state("");
  let menuFor = $state<string | null>(null);
  let renaming = $state<string | null>(null);
  let renameValue = $state("");

  const filteredProjects = $derived(
    search
      ? store.projects.filter((p) => {
          const q = search.toLowerCase();
          if (`${p.name} ${p.path}`.toLowerCase().includes(q)) return true;
          return store.stagesFor(p.id).some((s) => s.name.toLowerCase().includes(q));
        })
      : store.projects,
  );

  async function removeProject(id: string) {
    const p = store.projects.find((x) => x.id === id);
    if (!p) return;
    const n = store.stagesFor(id).length;
    menuFor = null;
    const ok = await confirm(
      `Remove "${p.name}" and its ${n} stage${n === 1 ? "" : "s"}? This only forgets them locally.`,
      { title: "Remove project", kind: "warning" },
    );
    if (ok) {
      store.removeProject(id);
      onToast(`Removed ${p.name}`);
    }
  }

  async function removeStage(s: Stage) {
    const ok = await confirm(
      `Remove stage "${s.name}"? This only forgets it locally.`,
      { title: "Remove stage", kind: "warning" },
    );
    if (ok) {
      store.removeStage(s.id);
      onToast(`Removed ${s.name}`);
    }
  }

  function startRename(p: Project) {
    renaming = p.id;
    renameValue = p.name;
    menuFor = null;
    setTimeout(() => document.querySelector<HTMLInputElement>(".proj-head .input")?.focus(), 0);
  }

  function commitRename(id: string) {
    store.renameProject(id, renameValue);
    renaming = null;
  }
</script>

<div class="dash">
  <div class="search-row">
    <div class="search">
      <span class="material-symbols-outlined">search</span>
      <input bind:value={search} placeholder="Search…" />
    </div>
    <button type="button" class="btn btn-primary btn-sm" onclick={onAddProject}>
      <span class="material-symbols-outlined" style="font-size: 16px">add</span> Add project
    </button>
  </div>

  {#each filteredProjects as p (p.id)}
    <div class="card proj">
      <div class="proj-head">
        {#if renaming === p.id}
          <input
            class="input mono"
            style="max-width: 220px; height: 28px"
            bind:value={renameValue}
            onkeydown={(e) => {
              if (e.key === "Enter") commitRename(p.id);
              else if (e.key === "Escape") renaming = null;
            }}
            onblur={() => {
              if (renaming === p.id) commitRename(p.id);
            }}
          />
        {:else}
          <button type="button" class="name-btn" title="Rename" onclick={() => startRename(p)}>
            <strong>{p.name}</strong>
          </button>
          <span class="dim mono path">{p.path}</span>
        {/if}
        <span class="spacer"></span>
        <button type="button" class="btn btn-ghost btn-sm" onclick={() => onAddStage(p)}>
          Add stage
        </button>
        <div class="menu-wrap">
          <button
            type="button"
            class="icon-btn"
            title="Project actions"
            onclick={() => {
              menuFor = menuFor === p.id ? null : p.id;
            }}
          >
            <span class="material-symbols-outlined">more_horiz</span>
          </button>
          {#if menuFor === p.id}
            <div
              class="catcher"
              role="button"
              tabindex="-1"
              aria-label="Close menu"
              onclick={() => (menuFor = null)}
              onkeydown={(e) => e.key === "Escape" && (menuFor = null)}
            ></div>
            <div class="menu card" role="menu">
              <button type="button" class="danger" onclick={() => removeProject(p.id)}>
                <span class="material-symbols-outlined">delete</span> Remove project…
              </button>
            </div>
          {/if}
        </div>
      </div>

      {#each store.stagesFor(p.id) as s (s.id)}
        <div class="stage-row">
          <button type="button" class="open" onclick={() => store.openStage(s.id)}>
            <span class="dot {s.lastError ? 'dot-err' : 'dot-ok'}"></span>
            <strong class="mono">{s.name}</strong>
            {#if s.lastError}<span class="badge badge-err">ERROR</span>{/if}
            <span class="dim mono meta">{s.profile ?? "default"} · {s.region ?? "—"}</span>
            <span class="spacer"></span>
            <span class="dim mono ago">{timeAgo(s.lastOpenedMs)}</span>
          </button>
          <button type="button" class="icon-btn" title="Edit" onclick={() => onEditStage(s)}>
            <span class="material-symbols-outlined">edit</span>
          </button>
          <button
            type="button"
            class="icon-btn pin-btn"
            title={s.pinned ? "Unpin" : "Pin"}
            onclick={() => store.togglePin(s.id)}
          >
            <span class="material-symbols-outlined">{s.pinned ? "push_pin" : "keep_off"}</span>
          </button>
          <button type="button" class="icon-btn danger-btn" title="Remove" onclick={() => removeStage(s)}>
            <span class="material-symbols-outlined">delete</span>
          </button>
        </div>
      {:else}
        <div class="empty-stages dim">
          No stages.
          <button type="button" class="link" onclick={() => onAddStage(p)}>Add stage</button>
        </div>
      {/each}
    </div>
  {:else}
    <div class="empty card">
      <p class="dim">No projects yet.</p>
      <button type="button" class="btn btn-primary btn-sm" onclick={onAddProject}>Add project</button>
    </div>
  {/each}
</div>

<style>
  .dash {
    height: 100%;
    overflow-y: auto;
    padding: 16px 22px 30px;
  }
  .search-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 16px;
  }
  .search {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--bg1);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 0 12px;
    height: 36px;
  }
  .search input {
    flex: 1;
    background: none;
    border: none;
    outline: none;
    color: var(--text);
    font-size: 13px;
  }
  .proj {
    margin-bottom: 10px;
  }
  .proj-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
  }
  .name-btn {
    background: none;
    border: none;
    color: var(--text);
    padding: 2px 4px;
    margin: -2px -4px;
    border-radius: 4px;
    cursor: pointer;
    transition: background 0.15s;
  }
  .name-btn:hover {
    background: var(--hover);
  }
  .menu-wrap {
    position: relative;
  }
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 20;
  }
  .menu {
    position: absolute;
    right: 0;
    top: calc(100% + 4px);
    z-index: 30;
    min-width: 180px;
    padding: 6px;
    background: var(--bg2);
    box-shadow:
      0 8px 32px -4px rgba(0, 0, 0, 0.7),
      0 0 0 1px rgba(255, 255, 255, 0.1);
  }
  .menu button {
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
    border-radius: 5px;
    cursor: pointer;
  }
  .menu button:hover {
    background: var(--hover);
  }
  .menu button.danger {
    color: var(--red-bright);
  }
  .menu button .material-symbols-outlined {
    font-size: 16px;
  }
  .path {
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .spacer {
    flex: 1;
  }
  .stage-row {
    display: flex;
    align-items: center;
    gap: 2px;
    border-bottom: 1px solid var(--border);
    padding: 4px 8px;
  }
  .proj > .stage-row:last-child {
    border-bottom: none;
  }
  .stage-row .open {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 10px;
    background: none;
    border: none;
    color: var(--text);
    font-size: 13px;
    text-align: left;
    padding: 8px;
    border-radius: 6px;
    min-width: 0;
    cursor: pointer;
    transition: background 0.15s;
  }
  .stage-row .open:hover {
    background: var(--hover);
  }
  .meta,
  .ago {
    font-size: 11px;
  }
  .icon-btn {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    color: var(--text-faint);
    border-radius: 6px;
    padding: 6px;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .icon-btn:hover {
    background: var(--hover);
    color: var(--text);
  }
  .icon-btn .material-symbols-outlined {
    font-size: 17px;
  }
  .icon-btn.pin-btn {
    color: var(--amber);
  }
  .icon-btn.danger-btn:hover {
    color: var(--red-bright);
  }
  .empty-stages {
    padding: 14px 16px;
    font-size: 13px;
  }
  .empty-stages .link {
    background: none;
    border: none;
    color: var(--amber);
    font-size: 13px;
    font-weight: 600;
    padding: 0;
    cursor: pointer;
    text-decoration: underline;
  }
  .empty {
    padding: 28px;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
  }
</style>
