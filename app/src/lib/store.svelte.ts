// Global UI state (Svelte 5 runes) + localStorage persistence.
// Projects, stages, pins and settings never leave the machine.

import type { Project, Settings, Stage, UiTheme } from "./types";

export type View = "welcome" | "dashboard" | "debugger" | "settings";

class Store {
  view = $state<View>("welcome");
  projects = $state<Project[]>([]);
  stages = $state<Stage[]>([]);
  activeStageId = $state<string | null>(null);
  paletteOpen = $state(false);
  addDialogOpen = $state(false);
  settings = $state<Settings>({
    tailIntervalSecs: 2,
    bufferLimit: 50_000,
    uiZoom: 1,
    uiTheme: "system",
  });
  pendingConstruct = $state<string | null>(null);
  /** ⌘K “open trace” — consumed by Debugger. */
  pendingTraceId = $state<string | null>(null);
  /** Deep link handoff — consumed by Debugger once the stage is loaded. */
  pendingFilter = $state<string | null>(null);
  pendingWindow = $state<{ startMs: number; endMs: number } | null>(null);
  booted = $state(false);
  #themeMql: MediaQueryList | null = null;

  boot() {
    if (this.booted) return;
    this.booted = true;
    try {
      const raw = localStorage.getItem("cwtrace.workspace.v1");
      if (raw) {
        const data = JSON.parse(raw) as {
          projects?: Project[];
          stages?: Stage[];
          settings?: Settings;
        };
        this.projects = data.projects ?? [];
        this.stages = data.stages ?? [];
        if (data.settings) this.settings = { ...this.settings, ...data.settings };
        this.settings.uiZoom = clampZoom(this.settings.uiZoom);
        if (!this.settings.uiTheme) this.settings.uiTheme = "system";
      }
    } catch {
      // Corrupt cache: start clean but keep pins safe by doing nothing destructive.
    }
    applyZoom(this.settings.uiZoom);
    this.applyTheme();
    this.view = this.projects.length > 0 ? "dashboard" : "welcome";
  }

  setTheme(theme: UiTheme) {
    this.settings.uiTheme = theme;
    this.applyTheme();
    this.save();
  }

  applyTheme() {
    if (typeof document === "undefined") return;
    const theme = this.settings.uiTheme ?? "system";
    const resolved =
      theme === "system"
        ? window.matchMedia("(prefers-color-scheme: light)").matches
          ? "light"
          : "dark"
        : theme;
    document.documentElement.setAttribute("data-theme", resolved);

    if (!this.#themeMql) {
      this.#themeMql = window.matchMedia("(prefers-color-scheme: light)");
      this.#themeMql.addEventListener("change", () => {
        if (this.settings.uiTheme === "system") this.applyTheme();
      });
    }
  }

  save() {
    try {
      localStorage.setItem(
        "cwtrace.workspace.v1",
        JSON.stringify({
          projects: this.projects,
          stages: this.stages,
          settings: this.settings,
        }),
      );
    } catch {
      // Storage full/blocked: session still works in memory.
    }
  }

  setZoom(z: number) {
    this.settings.uiZoom = clampZoom(z);
    applyZoom(this.settings.uiZoom);
    this.save();
  }

  pinnedStages(): Stage[] {
    return this.stages.filter((s) => s.pinned);
  }

  stagesFor(projectId: string): Stage[] {
    return this.stages.filter((s) => s.projectId === projectId);
  }

  projectName(projectId: string): string {
    return this.projects.find((p) => p.id === projectId)?.name ?? "unknown";
  }

  activeStage(): Stage | null {
    return this.stages.find((s) => s.id === this.activeStageId) ?? null;
  }

  openStage(id: string) {
    const stage = this.stages.find((s) => s.id === id);
    if (!stage) return;
    stage.lastOpenedMs = Date.now();
    stage.lastError = undefined;
    this.activeStageId = id;
    this.view = "debugger";
    this.save();
  }

  markStageError(id: string, message: string) {
    const stage = this.stages.find((s) => s.id === id);
    if (stage) {
      stage.lastError = message;
      this.save();
    }
  }

  togglePin(id: string) {
    const stage = this.stages.find((s) => s.id === id);
    if (stage) {
      stage.pinned = !stage.pinned;
      this.save();
    }
  }

  updateStage(id: string, patch: Partial<Stage>) {
    const stage = this.stages.find((s) => s.id === id);
    if (stage) {
      Object.assign(stage, patch);
      this.save();
    }
  }

  renameProject(id: string, name: string) {
    const project = this.projects.find((p) => p.id === id);
    if (project && name.trim()) {
      project.name = name.trim();
      this.save();
    }
  }

  removeStage(id: string) {
    const stage = this.stages.find((s) => s.id === id);
    if (!stage) return;
    this.stages = this.stages.filter((s) => s.id !== id);
    if (this.activeStageId === id) {
      this.activeStageId = null;
      if (this.view === "debugger") {
        this.view = this.projects.length ? "dashboard" : "welcome";
      }
    }
    this.save();
  }

  removeProject(id: string) {
    const doomed = new Set(this.stages.filter((s) => s.projectId === id).map((s) => s.id));
    this.projects = this.projects.filter((p) => p.id !== id);
    this.stages = this.stages.filter((s) => s.projectId !== id);
    if (this.activeStageId && doomed.has(this.activeStageId)) {
      this.activeStageId = null;
      if (this.view === "debugger") {
        this.view = this.projects.length ? "dashboard" : "welcome";
      }
    }
    if (this.projects.length === 0 && this.view !== "settings") {
      this.view = "welcome";
    }
    this.save();
  }
}

export const store = new Store();

export const ZOOM_MIN = 0.5;
export const ZOOM_MAX = 2.0;
export const ZOOM_STEP = 0.1;

function clampZoom(z: number): number {
  if (!Number.isFinite(z)) return 1;
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(z * 10) / 10));
}

/** Apply the zoom factor to the webview. Called on boot and on every change.
 * NOTE: CSS `zoom` scales content but leaves `vh` units absolute, so root
 * heights must be compensated via `--zoom` (see theme.css / App.svelte),
 * otherwise 100vh shells overflow and bottom-anchored UI is cut off. */
export function applyZoom(z: number) {
  if (typeof document !== "undefined") {
    const clamped = clampZoom(z);
    document.body.style.zoom = String(clamped);
    document.documentElement.style.setProperty("--zoom", String(clamped));
  }
}

export function zoomIn() {
  store.setZoom(store.settings.uiZoom + ZOOM_STEP);
}

export function zoomOut() {
  store.setZoom(store.settings.uiZoom - ZOOM_STEP);
}

export function zoomReset() {
  store.setZoom(1);
}

export function uid(): string {
  return Math.random().toString(36).slice(2, 10) + Date.now().toString(36);
}

export function timeAgo(ms?: number): string {
  if (!ms) return "never";
  const s = Math.max(1, Math.floor((Date.now() - ms) / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  if (d < 30) return `${d}d ago`;
  return `${Math.floor(d / 30)}mo ago`;
}
