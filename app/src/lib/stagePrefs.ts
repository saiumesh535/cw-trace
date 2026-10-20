/** Per-stage filter strings + last time range (localStorage). */

export type StagePrefs = {
  filter?: string;
  regexOn?: boolean;
  back?: string;
  customStart?: string;
  customEnd?: string;
  useCustom?: boolean;
  savedFilters?: string[];
};

const KEY = "cwtrace.stagePrefs.v1";

function all(): Record<string, StagePrefs> {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return {};
    return JSON.parse(raw) as Record<string, StagePrefs>;
  } catch {
    return {};
  }
}

export function loadStagePrefs(stageId: string): StagePrefs {
  return all()[stageId] ?? {};
}

export function saveStagePrefs(stageId: string, patch: Partial<StagePrefs>) {
  try {
    const map = all();
    const prev = map[stageId] ?? {};
    map[stageId] = { ...prev, ...patch };
    // Cap saved filter list.
    if (map[stageId].savedFilters) {
      map[stageId].savedFilters = [...new Set(map[stageId].savedFilters)]
        .filter(Boolean)
        .slice(0, 12);
    }
    localStorage.setItem(KEY, JSON.stringify(map));
  } catch {
    // ignore
  }
}

export function rememberFilter(stageId: string, filter: string) {
  const f = filter.trim();
  if (!f) return;
  const prev = loadStagePrefs(stageId);
  const saved = [f, ...(prev.savedFilters ?? []).filter((x) => x !== f)].slice(0, 12);
  saveStagePrefs(stageId, { filter: f, savedFilters: saved });
}
