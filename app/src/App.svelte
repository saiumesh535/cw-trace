<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api } from "./lib/api";
  import { parseDeepLink } from "./lib/deepLink";
  import { store, uid, zoomIn, zoomOut, zoomReset } from "./lib/store.svelte";
  import type { Project, ResourceNode, Stage } from "./lib/types";
  import Sidebar from "./components/Sidebar.svelte";
  import TopBar from "./components/TopBar.svelte";
  import CommandPalette from "./components/CommandPalette.svelte";
  import Welcome from "./components/Welcome.svelte";
  import Dashboard from "./components/Dashboard.svelte";
  import AddDialog from "./components/AddDialog.svelte";
  import Debugger from "./components/Debugger.svelte";
  import Settings from "./components/Settings.svelte";

  let paletteNodes = $state<ResourceNode[]>([]);
  let sampleBusy = $state(false);
  let toast = $state("");

  // Stage sheet state — three entry points share one dialog.
  let sheetOpen = $state(false);
  let sheetProject = $state<Project | null>(null);
  let sheetFolder = $state<string | null>(null);
  let sheetEdit = $state<Stage | null>(null);

  function showToast(msg: string) {
    toast = msg;
    setTimeout(() => (toast = ""), 3000);
  }

  function closeSheet() {
    sheetOpen = false;
    sheetProject = null;
    sheetFolder = null;
    sheetEdit = null;
  }

  /** Add project: folder picker first; sheet only after a folder is chosen. */
  async function openAddProject() {
    try {
      const picked = await api.pickFolder();
      if (typeof picked === "string" && picked) {
        sheetProject = null;
        sheetEdit = null;
        sheetFolder = picked;
        sheetOpen = true;
      }
    } catch (e) {
      showToast(String(e));
    }
  }

  function openAddStage(project: Project) {
    sheetFolder = null;
    sheetEdit = null;
    sheetProject = project;
    sheetOpen = true;
  }

  function openEditStage(stage: Stage) {
    sheetFolder = null;
    sheetProject = null;
    sheetEdit = stage;
    sheetOpen = true;
  }

  async function trySample() {
    if (sampleBusy) return;
    sampleBusy = true;
    try {
      const sample = await api.loadSampleGraph();
      let project = store.projects.find((p) => p.name === "sample-cdk");
      if (!project) {
        project = { id: uid(), name: "sample-cdk", path: sample.cdkOut, createdMs: Date.now() };
        store.projects = [...store.projects, project];
      }
      let stage = store.stages.find((s) => s.projectId === project.id);
      if (!stage) {
        stage = {
          id: uid(),
          projectId: project.id,
          name: "sample",
          cdkOut: sample.cdkOut,
          region: "us-east-1",
          pinned: false,
          lastOpenedMs: Date.now(),
        };
        store.stages = [...store.stages, stage];
      }
      store.save();
      store.openStage(stage.id);
      showToast("Saved sample");
    } catch (e) {
      showToast(String(e));
    } finally {
      sampleBusy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key.toLowerCase() === "k") {
      e.preventDefault();
      store.paletteOpen = !store.paletteOpen;
    } else if (mod && e.key.toLowerCase() === "o") {
      e.preventDefault();
      openAddProject();
    } else if (mod && e.key === ",") {
      e.preventDefault();
      store.view = "settings";
    } else if (mod && e.key === "1") {
      e.preventDefault();
      store.view = store.projects.length ? "dashboard" : "welcome";
    } else if (mod && /^[2-9]$/.test(e.key)) {
      const pin = store.pinnedStages()[Number(e.key) - 2];
      if (pin) {
        e.preventDefault();
        store.openStage(pin.id);
      }
    } else if (mod && (e.key === "=" || e.key === "+" || e.code === "NumpadAdd")) {
      e.preventDefault();
      zoomIn();
    } else if (mod && (e.key === "-" || e.key === "_" || e.code === "NumpadSubtract")) {
      e.preventDefault();
      zoomOut();
    } else if (mod && (e.key === "0" || e.code === "Numpad0")) {
      e.preventDefault();
      zoomReset();
    } else if (e.key === "Escape") {
      if (sheetOpen) closeSheet();
      else store.paletteOpen = false;
    }
  }

  /** WS7: route a `cw-trace://debug?...` link into the workspace. */
  function handleDeepLink(url: string) {
    const ctx = parseDeepLink(url);
    if (!ctx) return;
    const stage = ctx.stage
      ? (store.stages.find((s) => s.id === ctx.stage) ??
        store.stages.find((s) => s.name.toLowerCase() === ctx.stage!.toLowerCase()))
      : null;
    if (stage) {
      store.openStage(stage.id);
    } else if (store.activeStageId) {
      store.view = "debugger";
    } else {
      showToast(`No matching stage for link${ctx.stage ? `: ${ctx.stage}` : ""}`);
      return;
    }
    if (ctx.construct) store.pendingConstruct = ctx.construct;
    if (ctx.trace) store.pendingTraceId = ctx.trace;
    if (ctx.filter) store.pendingFilter = ctx.filter;
    if (ctx.startMs != null && ctx.endMs != null) {
      store.pendingWindow = { startMs: ctx.startMs, endMs: ctx.endMs };
    }
    showToast("Opened from link");
  }

  onMount(() => {
    store.boot();
    window.addEventListener("keydown", onKey);
    const unlistenDeep = listen<string>("deep-link-open", (e) => handleDeepLink(e.payload));
    return () => {
      window.removeEventListener("keydown", onKey);
      unlistenDeep.then((u) => u());
    };
  });
</script>

<div class="shell">
  <Sidebar onAddProject={openAddProject} />
  <div class="main">
    <TopBar />
    <div class="view">
      {#if store.view === "welcome"}
        <Welcome onOpenProject={openAddProject} onTrySample={trySample} {sampleBusy} />
      {:else if store.view === "dashboard"}
        <Dashboard
          onAddProject={openAddProject}
          onAddStage={openAddStage}
          onEditStage={openEditStage}
          onToast={showToast}
        />
      {:else if store.view === "debugger"}
        <Debugger onNodes={(n) => (paletteNodes = n)} />
      {:else}
        <Settings />
      {/if}
    </div>
  </div>
</div>

<CommandPalette
  graphNodes={paletteNodes}
  onOpenTrace={(id) => {
    store.pendingTraceId = id;
    if (store.activeStageId) store.view = "debugger";
  }}
/>
{#if sheetOpen}
  <AddDialog
    project={sheetProject}
    folderPath={sheetFolder}
    editStage={sheetEdit}
    onClose={closeSheet}
    onSaved={showToast}
  />
{/if}
{#if toast}<div class="toast mono" role="status" aria-live="polite">{toast}</div>{/if}

<style>
  .shell {
    display: flex;
    height: calc(100vh / var(--zoom, 1));
  }
  .main {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .view {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .toast {
    position: fixed;
    bottom: 18px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--bg2);
    border: 1px solid var(--border);
    color: var(--text);
    font-size: 12px;
    border-radius: 8px;
    padding: 10px 16px;
    z-index: 70;
  }
</style>
