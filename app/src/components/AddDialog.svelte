<script lang="ts">
  import { api } from "../lib/api";
  import { store, uid } from "../lib/store.svelte";
  import type { AwsProfile, CdkOutInspect, Project, Stage } from "../lib/types";
  import { onMount, tick } from "svelte";

  let {
    onClose,
    onSaved,
    project = null,
    folderPath = null,
    editStage = null,
  }: {
    onClose: () => void;
    onSaved?: (msg: string) => void;
    project?: Project | null;
    folderPath?: string | null;
    editStage?: Stage | null;
  } = $props();

  const editing = $derived(editStage !== null);
  const title = $derived(
    editing ? "Edit stage" : project ? `Add stage — ${project.name}` : "Add stage",
  );

  let folder = $state<string | null>(null);
  let folderName = $state("");
  let inspect = $state<CdkOutInspect | null>(null);
  let inspecting = $state(false);
  let stageName = $state("");
  let cdkOutDir = $state("");
  let profile = $state(store.settings.defaultProfile ?? "");
  let region = $state(store.settings.defaultRegion ?? "us-east-1");
  let stackFilter = $state("");
  let pin = $state(true);
  let saving = $state(false);
  let profiles = $state<AwsProfile[]>([]);
  let advancedOpen = $state(false);
  let synthesizing = $state(false);
  let synthErr = $state("");
  let testing = $state(false);
  let testMsg = $state("");
  let testOk = $state(false);
  let nameInput: HTMLInputElement | undefined = $state();

  onMount(async () => {
    try {
      profiles = await api.listAwsProfiles();
    } catch {
      profiles = [];
    }

    if (editStage) {
      const proj = store.projects.find((p) => p.id === editStage.projectId);
      folder = proj?.path ?? editStage.cdkOut;
      folderName = proj?.name ?? "";
      stageName = editStage.name;
      cdkOutDir = editStage.cdkOut;
      if (editStage.profile) profile = editStage.profile;
      if (editStage.region) region = editStage.region;
      stackFilter = editStage.stackFilter ?? "";
      pin = editStage.pinned;
      // Advanced open when non-default advanced values exist
      if (editStage.stackFilter || (proj && editStage.cdkOut !== proj.path)) {
        advancedOpen = true;
      }
    } else if (project) {
      folder = project.path;
      folderName = project.name;
    } else if (folderPath) {
      folder = folderPath;
      folderName = folderPath.replace(/\/$/, "").split("/").pop() || "project";
    }

    await tick();
    nameInput?.focus();
    nameInput?.select();
  });

  function onProfileChange(e: Event) {
    const name = (e.target as HTMLSelectElement).value;
    profile = name;
    const known = profiles.find((p) => p.name === name)?.region;
    if (known && (!region || region === "us-east-1" || region === store.settings.defaultRegion)) {
      region = known;
    }
  }

  async function runInspect(dir: string) {
    inspecting = true;
    inspect = null;
    try {
      inspect = await api.inspectCdkOut(dir);
      if (inspect && !inspect.error) {
        if (!cdkOutDir) cdkOutDir = inspect.outdir;
        if (!stageName) stageName = inspect.stageHints[0] ?? "dev";
      }
    } catch (e) {
      inspect = {
        cdkJsonFound: false,
        outdir: dir,
        stacks: [],
        stageHints: ["dev", "staging", "prod"],
        nodeCount: 0,
        logGroupCount: 0,
        stale: false,
        error: String(e),
      };
    } finally {
      inspecting = false;
    }
  }

  async function synthesizeNow() {
    if (!folder || synthesizing) return;
    synthesizing = true;
    synthErr = "";
    try {
      const res = await api.runSynth(folder);
      cdkOutDir = res.outdir;
      await runInspect(folder);
    } catch (e) {
      synthErr = String(e);
    } finally {
      synthesizing = false;
    }
  }

  async function testConnection() {
    testing = true;
    testMsg = "";
    testOk = false;
    try {
      const res = await api.testConnection({
        profile: profile || undefined,
        region: region || undefined,
      });
      testOk = true;
      testMsg = `OK — ${res.profile} · ${res.region}`;
    } catch (e) {
      testOk = false;
      testMsg = String(e);
      // Loudly surface missing/typo'd profiles from ~/.aws/config.
      if (profiles.length > 0 && profile && !profiles.some((p) => p.name === profile)) {
        testMsg += ` · known profiles: ${profiles.map((p) => p.name).slice(0, 8).join(", ")}`;
      }
    } finally {
      testing = false;
    }
  }

  $effect(() => {
    if (folder) runInspect(folder);
  });

  async function save() {
    if (!stageName.trim() || saving) return;
    saving = true;

    if (editing && editStage) {
      store.updateStage(editStage.id, {
        name: stageName.trim(),
        cdkOut: cdkOutDir || editStage.cdkOut,
        profile: profile.trim() || undefined,
        region: region.trim() || undefined,
        stackFilter: stackFilter.trim() || undefined,
        pinned: pin,
        lastError: undefined,
      });
      onSaved?.(`Saved ${stageName.trim()}`);
      onClose();
      return;
    }

    if (!folder) return;

    let proj = project;
    if (!proj) {
      proj =
        store.projects.find((p) => p.path === folder) ??
        (() => {
          const p = {
            id: uid(),
            name: folderName || "project",
            path: folder!,
            createdMs: Date.now(),
          };
          store.projects = [...store.projects, p];
          return p;
        })();
    }

    const stage: Stage = {
      id: uid(),
      projectId: proj.id,
      name: stageName.trim(),
      cdkOut: cdkOutDir || inspect?.outdir || folder,
      profile: profile.trim() || undefined,
      region: region.trim() || undefined,
      stackFilter: stackFilter.trim() || undefined,
      pinned: pin,
      lastOpenedMs: Date.now(),
    };
    store.stages = [...store.stages, stage];
    store.save();
    store.openStage(stage.id);
    onSaved?.(`Saved ${stage.name}`);
    onClose();
  }
</script>

<div
  class="overlay"
  role="button"
  tabindex="-1"
  aria-label="Cancel"
  onclick={onClose}
  onkeydown={(e) => e.key === "Escape" && onClose()}
>
  <div
    class="dialog card"
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    aria-label={title}
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
  >
    <div class="label">{title}</div>
    {#if folder}
      <div class="path mono dim">{folder}</div>
    {/if}

    {#if inspecting}
      <div class="dim mono status">Inspecting cdk.out…</div>
    {:else if inspect?.error}
      <div class="err mono" role="alert">{inspect.error}</div>
    {:else if inspect}
      <div class="ok mono">
        {inspect.nodeCount} constructs · {inspect.logGroupCount} log groups
        {#if inspect.stale}
          <span class="warn-inline"> · cdk.out may be stale</span>
        {/if}
      </div>
    {/if}

    <div class="field">
      <label class="flabel" for="stage-name">Stage name</label>
      <input
        id="stage-name"
        class="input"
        bind:this={nameInput}
        bind:value={stageName}
        placeholder="dev"
        onkeydown={(e) => e.key === "Enter" && save()}
      />
      {#if inspect && inspect.stageHints.length > 0}
        <div class="matches mono">
          <span class="dim">Detected:</span>
          {#each inspect.stageHints as h (h)}
            <button type="button" class="chip" onclick={() => (stageName = h)}>{h}</button>
          {/each}
        </div>
      {/if}
    </div>

    <div class="conn-row">
      <button
        type="button"
        class="btn btn-ghost btn-sm"
        disabled={testing}
        onclick={testConnection}
      >
        {testing ? "Testing…" : "Test connection"}
      </button>
      {#if testMsg}
        <span class="test-msg mono" class:ok={testOk} class:bad={!testOk}>{testMsg}</span>
      {/if}
    </div>
    {#if profiles.length > 0 && profile && !profiles.some((p) => p.name === profile)}
      <div class="warn" role="alert">
        Profile <span class="mono">{profile}</span> not in ~/.aws/config. Pick one:
        {profiles.slice(0, 6).map((p) => p.name).join(", ")}
      </div>
    {/if}

    <div class="two">
      <div class="field">
        <label class="flabel" for="aws-profile">AWS profile</label>
        <select
          id="aws-profile"
          class="input mono"
          value={profile}
          onchange={onProfileChange}
        >
          <option value="">default chain</option>
          {#each profiles as p (p.name)}
            <option value={p.name}>{p.name}{p.region ? ` — ${p.region}` : ""}</option>
          {/each}
        </select>
      </div>
      <div class="field">
        <label class="flabel" for="aws-region">Region</label>
        <input id="aws-region" class="input mono" bind:value={region} placeholder="us-east-1" />
      </div>
    </div>

    <button type="button" class="pinrow" onclick={() => (pin = !pin)}>
      <span class="material-symbols-outlined" style="color: var(--amber)">push_pin</span>
      <span
        ><strong>Pin this stage</strong><br /><span class="dim"
          >Shortcut in sidebar</span
        ></span
      >
      <span class="spacer"></span>
      <span class="toggle" class:on={pin} aria-hidden="true"><i></i></span>
    </button>

    <details class="advanced" bind:open={advancedOpen}>
      <summary>Advanced</summary>
      <div class="field">
        <label class="flabel" for="cdk-out">cdk.out directory</label>
        <input id="cdk-out" class="input mono" bind:value={cdkOutDir} />
        <div class="synth-row">
          <button
            type="button"
            class="btn btn-ghost btn-sm"
            disabled={!folder || synthesizing || inspecting}
            onclick={synthesizeNow}
          >
            {synthesizing ? "Synthesizing…" : "Synthesize now"}
          </button>
          {#if inspect?.stale}
            <span class="warn-inline" role="alert">
              <span class="material-symbols-outlined">warning</span>
              cdk.out older than sources
            </span>
          {/if}
        </div>
        {#if synthErr}
          <div class="warn" role="alert">{synthErr}</div>
        {/if}
      </div>
      <div class="field">
        <label class="flabel" for="stack-filter">Stack filter <span class="dim">optional</span></label>
        <input
          id="stack-filter"
          class="input mono"
          bind:value={stackFilter}
          placeholder="e.g. OrdersServiceStack-*"
        />
      </div>
    </details>

    <div class="actions">
      <button type="button" class="btn btn-ghost" onclick={onClose}>Cancel <kbd>Esc</kbd></button>
      <span class="spacer"></span>
      <button
        type="button"
        class="btn btn-primary"
        onclick={save}
        disabled={!stageName.trim() || saving || !folder}
      >
        {saving ? "Saving…" : editing ? "Save" : "Add stage & open"}
      </button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.65);
    z-index: 50;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: 6vh 16px 16px;
    overflow-y: auto;
  }
  .dialog {
    width: 480px;
    max-width: 100%;
    background: var(--bg1);
    padding: 20px 22px;
    box-shadow:
      0 8px 32px -4px rgba(0, 0, 0, 0.7),
      0 0 0 1px rgba(255, 255, 255, 0.1);
  }
  .label {
    margin-bottom: 4px;
  }
  .path {
    font-size: 11px;
    margin-bottom: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .status {
    font-size: 12px;
    margin-bottom: 12px;
  }
  .err {
    font-size: 12px;
    color: var(--red-bright);
    margin-bottom: 12px;
  }
  .ok {
    font-size: 11px;
    color: var(--green-bright);
    margin-bottom: 12px;
  }
  .warn-inline {
    color: var(--amber);
  }
  .field {
    margin-bottom: 12px;
  }
  .flabel {
    display: block;
    font-size: 12px;
    font-weight: 600;
    margin-bottom: 6px;
  }
  .flabel .dim {
    font-weight: 400;
    font-size: 11px;
  }
  .matches {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    font-size: 11px;
    margin-top: 6px;
  }
  .chip {
    background: var(--bg2);
    border: 1px solid var(--border);
    color: var(--text-dim);
    border-radius: 4px;
    font-family: var(--font-mono);
    font-size: 11px;
    padding: 2px 8px;
    cursor: pointer;
    transition: color 0.15s, border-color 0.15s;
  }
  .chip:hover {
    color: var(--text);
    border-color: var(--amber);
  }
  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .pinrow {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    text-align: left;
    background: var(--bg2);
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text);
    padding: 10px 12px;
    font-size: 12px;
    margin: 4px 0 12px;
    cursor: pointer;
    transition: background 0.15s;
  }
  .pinrow:hover {
    background: var(--hover);
  }
  .spacer {
    flex: 1;
  }
  .toggle {
    width: 36px;
    height: 20px;
    border-radius: 9999px;
    background: #2b3546;
    position: relative;
    flex: none;
    transition: background 0.15s;
  }
  .toggle i {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 9999px;
    background: var(--text-dim);
    transition: left 0.15s;
  }
  .toggle.on {
    background: var(--amber);
  }
  .toggle.on i {
    left: 18px;
    background: var(--on-amber);
  }
  .advanced {
    margin-bottom: 14px;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0 12px;
    background: var(--bg2);
  }
  .advanced summary {
    cursor: pointer;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-dim);
    padding: 10px 0;
    list-style: none;
  }
  .advanced summary::-webkit-details-marker {
    display: none;
  }
  .advanced summary::before {
    content: "▸ ";
    color: var(--text-faint);
  }
  .advanced[open] summary::before {
    content: "▾ ";
  }
  .advanced[open] summary {
    border-bottom: 1px solid var(--border);
    margin-bottom: 10px;
  }
  .conn-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 4px 0 12px;
    flex-wrap: wrap;
  }
  .test-msg {
    font-size: 11px;
    color: var(--text-dim);
    max-width: 100%;
    word-break: break-word;
  }
  .test-msg.ok {
    color: var(--sage, #6b8f71);
  }
  .test-msg.bad {
    color: var(--red-bright, #e07070);
  }
  .synth-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 8px;
    flex-wrap: wrap;
  }
  .warn-inline {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--amber);
  }
  .warn-inline .material-symbols-outlined {
    font-size: 14px;
  }
  .warn {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    background: rgba(249, 115, 22, 0.08);
    border: 1px solid rgba(249, 115, 22, 0.3);
    border-radius: 6px;
    padding: 8px 10px;
    font-size: 12px;
    margin-top: 8px;
  }
  .warn .material-symbols-outlined {
    color: var(--amber);
    font-size: 16px;
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }
</style>
