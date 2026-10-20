<script lang="ts">
  import { store } from "../lib/store.svelte";
  import type { UiTheme } from "../lib/types";

  let section = $state("aws");

  const themes: { id: UiTheme; label: string }[] = [
    { id: "system", label: "System" },
    { id: "light", label: "Light" },
    { id: "dark", label: "Dark" },
  ];
</script>

<div class="settings">
  <div class="head">
    <div>
      <div class="label">Preferences</div>
      <h2>Settings</h2>
    </div>
    <span class="spacer"></span>
    <span class="badge badge-info">Local only</span>
  </div>

  <div class="cols">
    <nav class="card">
      {#each [
        ["aws", "AWS"],
        ["tail", "Live Tail"],
        ["appearance", "Appearance"],
        ["pins", "Pinned stages"],
        ["about", "About"],
      ] as [id, label] (id)}
        <button type="button" class:active={section === id} onclick={() => (section = id)}>
          {label}
        </button>
      {/each}
    </nav>

    <div class="body">
      {#if section === "aws"}
        <div class="card sec">
          <h3>
            AWS credentials
            <span class="dim mono" style="font-size: 11px">~/.aws/config</span>
          </h3>
          <div class="two">
            <div>
              <label class="label" for="default-profile">Default AWS profile</label>
              <input
                id="default-profile"
                class="input mono"
                bind:value={store.settings.defaultProfile}
                placeholder="default chain"
                onchange={() => store.save()}
              />
            </div>
            <div>
              <label class="label" for="default-region">Default region</label>
              <input
                id="default-region"
                class="input mono"
                bind:value={store.settings.defaultRegion}
                placeholder="us-east-1"
                onchange={() => store.save()}
              />
            </div>
          </div>
          <div class="note">
            <span class="material-symbols-outlined">shield</span>
            AWS SSO uses the local credential cache — no credentials ever leave this machine.
          </div>
        </div>
      {:else if section === "tail"}
        <div class="card sec">
          <h3>Live Tail</h3>
          <div class="two">
            <div>
              <label class="label" for="tail-interval"
                >Polling interval ({store.settings.tailIntervalSecs}s)</label
              >
              <input
                id="tail-interval"
                type="range"
                min="1"
                max="10"
                bind:value={store.settings.tailIntervalSecs}
                onchange={() => store.save()}
              />
            </div>
            <div>
              <label class="label" for="buffer-limit"
                >Log buffer limit ({store.settings.bufferLimit.toLocaleString()} rows)</label
              >
              <input
                id="buffer-limit"
                type="range"
                min={1000}
                max={100000}
                step={1000}
                bind:value={store.settings.bufferLimit}
                onchange={() => store.save()}
              />
            </div>
          </div>
          <div class="dim" style="font-size: 11px; margin-top: 8px">
            Tail emits batched <span class="mono">log-batch</span> events.
          </div>
        </div>
      {:else if section === "appearance"}
        <div class="card sec">
          <h3>Theme</h3>
          <div class="theme-row" role="radiogroup" aria-label="Theme">
            {#each themes as t (t.id)}
              <button
                type="button"
                class="theme-btn"
                class:on={store.settings.uiTheme === t.id}
                aria-checked={store.settings.uiTheme === t.id}
                role="radio"
                onclick={() => store.setTheme(t.id)}
              >
                {t.label}
              </button>
            {/each}
          </div>
          <p class="dim" style="font-size: 12px; margin: 12px 0 0">
            Warm charcoal and amber — no blue. Light uses paper tones and deeper amber for contrast.
          </p>
        </div>
      {:else if section === "pins"}
        <div class="card sec">
          <h3>
            Pinned stages
            <span class="dim mono" style="font-size: 11px"
              >{store.pinnedStages().length} of 9 slots</span
            >
          </h3>
          {#each store.pinnedStages() as s, i (s.id)}
            <div class="pinrow mono">
              <span class="dim">⌘{i + 2}</span>
              <span class="dot {s.lastError ? 'dot-err' : 'dot-ok'}"></span>
              <span>{s.name} — {store.projectName(s.projectId)}</span>
              <span class="badge badge-dim">profile: {s.profile ?? "default"}</span>
              <span class="spacer"></span>
              <span class="dim">{s.region}</span>
              <button type="button" class="btn btn-ghost btn-sm" onclick={() => store.togglePin(s.id)}
                >Unpin</button
              >
            </div>
          {:else}
            <div class="dim">No pins yet — pin a stage from any stage row.</div>
          {/each}
        </div>
      {:else}
        <div class="card sec">
          <h3>cw-trace <span class="badge badge-warn">v0.1.0</span></h3>
          <p class="dim">
            Local, read-only debugger for CDK-deployed services. Never modifies AWS, never phones
            home, no accounts.
          </p>
          <p class="mono dim" style="font-size: 11px">
            Runtime: Tauri 2 / Rust core · Frontend: Svelte 5 + Vite
          </p>
          <h3 style="margin-top: 18px">Packaging</h3>
          <p class="dim" style="font-size: 12px; line-height: 1.45">
            Ship a desktop build from <span class="mono">app/</span>:
          </p>
          <pre class="mono pack">cd app && pnpm tauri build</pre>
          <p class="dim" style="font-size: 12px; line-height: 1.45; margin-top: 10px">
            Auto-updater is scaffolded off until you set a signed update endpoint and pubkey in
            <span class="mono">tauri.conf.json</span> (FR-24). Check for updates stays local-only
            until then.
          </p>
          <button
            type="button"
            class="btn btn-ghost btn-sm"
            style="margin-top: 8px"
            onclick={() => alert("Auto-updater not configured — no endpoint yet.")}
          >
            Check for updates
          </button>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .settings {
    height: 100%;
    overflow-y: auto;
    padding: 18px 24px 30px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 16px;
  }
  .head h2 {
    margin: 2px 0 0;
    font-size: 20px;
  }
  .spacer {
    flex: 1;
  }
  .cols {
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr);
    gap: 14px;
    align-items: start;
  }
  nav.card {
    padding: 8px;
    position: sticky;
    top: 0;
  }
  nav button {
    display: flex;
    justify-content: space-between;
    width: 100%;
    background: none;
    border: none;
    color: var(--text-dim);
    font-size: 13px;
    padding: 8px 10px;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  nav button:hover {
    background: var(--hover);
    color: var(--text);
  }
  nav button.active {
    background: var(--bg2);
    color: var(--text);
  }
  .sec {
    padding: 16px;
    margin-bottom: 14px;
  }
  .sec h3 {
    margin: 0 0 12px;
    font-size: 14px;
  }
  .two {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  .note {
    display: flex;
    gap: 8px;
    align-items: flex-start;
    font-size: 12px;
    color: var(--text-dim);
    margin-top: 12px;
  }
  .note .material-symbols-outlined {
    color: var(--sage);
    font-size: 17px;
  }
  input[type="range"] {
    width: 100%;
    accent-color: var(--amber);
  }
  .pinrow {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12px;
    padding: 8px 4px;
    border-bottom: 1px solid var(--border);
  }
  .theme-row {
    display: flex;
    gap: 8px;
  }
  .pack {
    margin: 8px 0 0;
    padding: 10px 12px;
    background: var(--bg2);
    border: 1px solid var(--border);
    border-radius: 6px;
    font-size: 11px;
    overflow-x: auto;
  }
  .theme-btn {
    flex: 1;
    height: 36px;
    background: var(--bg2);
    border: 1px solid var(--border);
    color: var(--text-dim);
    border-radius: 6px;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
  }
  .theme-btn:hover {
    color: var(--text);
    background: var(--hover);
  }
  .theme-btn.on {
    color: var(--on-amber);
    background: var(--amber);
    border-color: var(--amber);
  }
</style>
