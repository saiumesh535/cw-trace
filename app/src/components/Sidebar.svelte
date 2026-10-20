<script lang="ts">
  import { store } from "../lib/store.svelte";
  import logo from "../assets/logo.png";

  let { onAddProject }: { onAddProject: () => void } = $props();

  const pinned = $derived(store.pinnedStages());

  function goHome() {
    store.view = store.projects.length ? "dashboard" : "welcome";
  }

  // Which projects are expanded to show their stages. Defaults to the
  // project holding the active stage.
  let expanded = $state<Record<string, boolean>>({});

  function isExpanded(id: string): boolean {
    if (expanded[id] !== undefined) return expanded[id];
    return store.stagesFor(id).some((s) => s.id === store.activeStageId);
  }
</script>

<aside class="sidebar">
  <button class="brand" onclick={goHome} title="Home (⌘1)">
    <img class="logo" src={logo} alt="" width="22" height="22" />
    <strong>cw-trace</strong>
  </button>

  <nav>
    <button
      class="nav-item"
      class:active={store.view === "dashboard" || store.view === "welcome"}
      onclick={() => (store.view = store.projects.length ? "dashboard" : "welcome")}
    >
      <span class="material-symbols-outlined">home</span> Home <kbd>⌘1</kbd>
    </button>

    {#if pinned.length > 0}
      <div class="label" style="margin: 12px 8px 4px">Pinned</div>
      {#each pinned as s, i (s.id)}
        <button
          class="nav-item"
          class:active={store.activeStageId === s.id && store.view === "debugger"}
          onclick={() => store.openStage(s.id)}
          title={`${store.projectName(s.projectId)} — ${s.region ?? ""}`}
        >
          <span class="dot {s.lastError ? 'dot-err' : 'dot-ok'}"></span>
          <span class="truncate">{s.name} — {store.projectName(s.projectId)}</span>
          {#if i < 8}<kbd>⌘{i + 2}</kbd>{/if}
        </button>
      {/each}
    {/if}

    <div class="label" style="margin: 12px 8px 4px">Projects</div>
    {#each store.projects as p (p.id)}
      <button
        class="nav-item"
        title={p.path}
        aria-expanded={isExpanded(p.id)}
        onclick={() => (expanded[p.id] = !isExpanded(p.id))}
      >
        <span class="material-symbols-outlined chev" class:open={isExpanded(p.id)}>chevron_right</span>
        <span class="truncate">{p.name}</span>
        <span class="dim mono" style="font-size: 10px"
          >{store.stagesFor(p.id).length}</span
        >
      </button>
      {#if isExpanded(p.id)}
        {#each store.stagesFor(p.id) as s (s.id)}
          <button
            class="nav-item sub"
            class:active={store.activeStageId === s.id && store.view === "debugger"}
            title={`Open ${s.name} (${s.region ?? "default region"})`}
            onclick={() => store.openStage(s.id)}
          >
            <span class="dot {s.lastError ? 'dot-err' : 'dot-ok'}"></span>
            <span class="truncate">{s.name}</span>
          </button>
        {/each}
      {/if}
    {/each}
    <button class="nav-item" onclick={onAddProject}>
      <span class="material-symbols-outlined">add</span> Add project <kbd>⌘O</kbd>
    </button>
  </nav>

  <div class="foot">
    <button
      class="nav-item"
      class:active={store.view === "settings"}
      onclick={() => (store.view = "settings")}
    >
      <span class="material-symbols-outlined">settings</span> Settings <kbd>⌘,</kbd>
    </button>
  </div>
</aside>

<style>
  .sidebar {
    width: 240px;
    flex: none;
    align-self: stretch;
    background: var(--bg1);
    border-right: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 10px;
    background: none;
    border: none;
    color: var(--text);
    font-size: 14px;
    text-align: left;
    border-radius: 6px;
    margin: 2px 4px 0;
  }
  .brand:hover {
    background: var(--hover);
  }
  .logo {
    width: 22px;
    height: 22px;
    border-radius: 5px;
    flex: none;
  }
  nav {
    flex: 1;
    overflow-y: auto;
    padding: 0 8px;
  }
  .nav-item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    text-align: left;
    background: none;
    border: none;
    color: var(--text-dim);
    font-size: 13px;
    padding: 7px 10px;
    border-radius: 6px;
  }
  .nav-item:hover {
    background: var(--hover);
    color: var(--text);
  }
  .nav-item.active {
    background: var(--bg2);
    color: var(--text);
  }
  .nav-item .material-symbols-outlined {
    font-size: 17px;
  }
  .nav-item .chev {
    font-size: 15px;
    transition: transform 0.12s;
    color: var(--text-faint);
  }
  .nav-item .chev.open {
    transform: rotate(90deg);
  }
  .nav-item.sub {
    padding-left: 30px;
    font-size: 12px;
  }
  .nav-item kbd {
    margin-left: auto;
  }
  .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
  }
  .foot {
    padding: 8px;
    border-top: 1px solid var(--border);
  }
</style>
