<script lang="ts">
  let {
    onOpenProject,
    onTrySample,
    sampleBusy = false,
  }: {
    onOpenProject: () => void;
    onTrySample: () => void;
    sampleBusy?: boolean;
  } = $props();
</script>

<div class="welcome">
  <h1>Stop hunting for log groups.</h1>
  <p>
    Point at a CDK project, pick a stage, and jump between CloudWatch logs and X-Ray traces.
  </p>
  <div class="cta">
    <button type="button" class="btn btn-primary" onclick={onOpenProject}>
      <span class="material-symbols-outlined">folder_open</span> Open CDK project <kbd>⌘O</kbd>
    </button>
    <button type="button" class="btn btn-ghost" onclick={onTrySample} disabled={sampleBusy}>
      <span class="material-symbols-outlined">play_circle</span>
      {sampleBusy ? "Loading…" : "Try sample"}
    </button>
  </div>
  <p class="reassure dim mono">
    <span class="material-symbols-outlined">lock</span> Read-only. Local only.
  </p>
</div>

<style>
  .welcome {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    justify-content: center;
    padding: 48px 56px;
    max-width: 560px;
  }
  h1 {
    margin: 0 0 12px;
    font-size: 28px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }
  p {
    margin: 0 0 24px;
    font-size: 15px;
    line-height: 1.5;
    color: var(--text-dim);
  }
  .cta {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
  }
  .reassure {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 28px;
    font-size: 11px;
  }
  .reassure .material-symbols-outlined {
    font-size: 14px;
    color: var(--sage);
  }
</style>
