// WS7: shareable `cw-trace://debug?...` links.
// The Rust side registers the scheme and re-emits incoming URLs as the
// `deep-link-open` event (see app/src-tauri/src/lib.rs).

export interface DeepLinkContext {
  /** Stage id or (case-insensitive) stage name. */
  stage?: string;
  construct?: string;
  trace?: string;
  filter?: string;
  startMs?: number;
  endMs?: number;
}

export function buildDeepLink(ctx: DeepLinkContext): string {
  const q = new URLSearchParams();
  if (ctx.stage) q.set("stage", ctx.stage);
  if (ctx.construct) q.set("construct", ctx.construct);
  if (ctx.trace) q.set("trace", ctx.trace);
  if (ctx.filter) q.set("filter", ctx.filter);
  if (ctx.startMs != null && Number.isFinite(ctx.startMs)) {
    q.set("start", String(Math.trunc(ctx.startMs)));
  }
  if (ctx.endMs != null && Number.isFinite(ctx.endMs)) {
    q.set("end", String(Math.trunc(ctx.endMs)));
  }
  const qs = q.toString();
  return `cw-trace://debug${qs ? `?${qs}` : ""}`;
}

export function parseDeepLink(url: string): DeepLinkContext | null {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  if (parsed.protocol !== "cw-trace:") return null;
  const q = parsed.searchParams;
  const ctx: DeepLinkContext = {};
  const stage = q.get("stage");
  const construct = q.get("construct");
  const trace = q.get("trace");
  const filter = q.get("filter");
  if (stage) ctx.stage = stage;
  if (construct) ctx.construct = construct;
  if (trace) ctx.trace = trace;
  if (filter) ctx.filter = filter;
  const start = Number(q.get("start"));
  const end = Number(q.get("end"));
  if (Number.isFinite(start) && start > 0) ctx.startMs = start;
  if (Number.isFinite(end) && end > 0) ctx.endMs = end;
  return ctx;
}
