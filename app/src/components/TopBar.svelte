<script lang="ts">
  import { store, zoomIn, zoomOut, zoomReset } from "../lib/store.svelte";
  import logo from "../assets/logo.png";
</script>

<header class="topbar">
  <div class="left">
    <img class="logo" src={logo} alt="" width="20" height="20" />
    <span class="title">cw-trace</span>
  </div>
  <div class="right">
    <button type="button" class="palette-btn" onclick={() => (store.paletteOpen = true)}>
      <span class="material-symbols-outlined">search</span> Jump <kbd>⌘K</kbd>
    </button>
    <div class="zoom" role="group" aria-label="Webview zoom">
      <button type="button" onclick={zoomOut} aria-label="Zoom out" title="Zoom out (⌘-)">−</button>
      <button type="button" class="pct mono" onclick={zoomReset} title="Reset zoom (⌘0)">
        {Math.round(store.settings.uiZoom * 100)}%
      </button>
      <button type="button" onclick={zoomIn} aria-label="Zoom in" title="Zoom in (⌘+)">+</button>
    </div>
  </div>
</header>

<style>
  .topbar {
    height: 44px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 14px;
    background: var(--bg1);
    border-bottom: 1px solid var(--border);
  }
  .left {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .logo {
    width: 20px;
    height: 20px;
    border-radius: 4px;
  }
  .title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-dim);
  }
  .right {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .palette-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: var(--bg2);
    border: 1px solid var(--border);
    color: var(--text-dim);
    border-radius: 6px;
    font-size: 12px;
    padding: 6px 10px;
    cursor: pointer;
    transition: color 0.15s, background 0.15s;
  }
  .palette-btn:hover {
    color: var(--text);
    background: var(--hover);
  }
  .palette-btn .material-symbols-outlined {
    font-size: 15px;
  }
  .zoom {
    display: inline-flex;
    align-items: center;
    background: var(--bg2);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .zoom button {
    background: none;
    border: none;
    color: var(--text-dim);
    font-size: 14px;
    line-height: 1;
    padding: 6px 9px;
    cursor: pointer;
  }
  .zoom button:hover {
    color: var(--text);
    background: var(--hover);
  }
  .zoom .pct {
    font-size: 11px;
    min-width: 44px;
    text-align: center;
    color: var(--text-dim);
  }
</style>
