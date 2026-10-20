import type { ResourceNode, XRaySubsegment, XRayTrace } from "./types";

/** X-Ray trace ID: `1-<8 hex>-<24 hex>` (35 chars). */
const TRACE_RE = /(?<![0-9a-fA-F])1-[0-9a-fA-F]{8}-[0-9a-fA-F]{24}(?![0-9a-fA-F])/g;
const UUID_RE =
  /(?<![0-9a-fA-F-])[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}(?![0-9a-fA-F-])/g;

export function isTraceId(value: string): boolean {
  return /^1-[0-9a-fA-F]{8}-[0-9a-fA-F]{24}$/.test(value.trim());
}

/**
 * Map an X-Ray span (segment or subsegment) name to a construct.
 * Lambda segments carry the function name; AWS subsegments may match a
 * data-plane `resource_name` (table, queue, …).
 */
export function matchConstructForSpan(
  spanName: string,
  nodes: ResourceNode[],
): ResourceNode | null {
  const name = spanName.trim();
  if (!name) return null;
  const byFn = nodes.find((n) => n.function_name === name);
  if (byFn) return byFn;
  const byResource = nodes.find((n) => n.resource_name === name);
  if (byResource) return byResource;
  const byLeaf = nodes.find((n) => n.construct_path.split("/").pop() === name);
  if (byLeaf) return byLeaf;
  const byGroup = nodes.find((n) => n.log_group?.endsWith(`/${name}`));
  if (byGroup) return byGroup;
  // Soft match: span name contains the table/queue name (common for AWS SDKs).
  const byContains = nodes.find(
    (n) =>
      !!n.resource_name &&
      n.resource_name.length >= 3 &&
      name.toLowerCase().includes(n.resource_name.toLowerCase()),
  );
  return byContains ?? null;
}

/** Ordered unique construct paths visited by a trace (for the live map path). */
export function constructsAlongTrace(trace: XRayTrace, nodes: ResourceNode[]): string[] {
  const path: string[] = [];
  const visit = (name: string) => {
    const n = matchConstructForSpan(name, nodes);
    if (n && path[path.length - 1] !== n.construct_path) {
      path.push(n.construct_path);
    }
  };
  const walkSubs = (subs?: XRaySubsegment[]) => {
    for (const s of subs ?? []) {
      visit(s.name);
      walkSubs(s.subsegments);
    }
  };
  for (const seg of trace.segments) {
    visit(seg.name);
    walkSubs(seg.subsegments);
  }
  return path;
}

/** Map column for construct-map layout. */
export function mapColumn(kind: string): "in" | "compute" | "data" | null {
  switch (kind) {
    case "rest-api":
    case "http-api":
    case "api-stage":
    case "event-bridge-rule":
      return "in";
    case "lambda-function":
    case "ecs-service":
    case "ecs-task-definition":
    case "state-machine":
      return "compute";
    case "dynamo-db-table":
    case "sqs-queue":
    case "sns-topic":
    case "s3-bucket":
    case "kinesis-stream":
      return "data";
    default:
      return null;
  }
}

export type LogSeverity = "ERROR" | "WARN" | "START" | "END" | "INFO";

const JSON_LEVEL_RE =
  /"(?:level|levelname|severity)"\s*:\s*"(trace|debug|info|notice|warn|warning|error|fatal|critical)"/i;

/** Best-effort severity for a log message (plain text + JSON logs). */
export function severityOf(msg: string): LogSeverity {
  const level = JSON_LEVEL_RE.exec(msg)?.[1]?.toLowerCase();
  if (level) {
    if (level === "error" || level === "fatal" || level === "critical") return "ERROR";
    if (level === "warn" || level === "warning") return "WARN";
    return "INFO";
  }
  if (/\b(ERROR|FATAL|CRITICAL)\b|\bFault\b|\bThrottl/.test(msg) || /"error"\s*:/.test(msg)) {
    return "ERROR";
  }
  if (/\b(WARN|WARNING)\b/.test(msg) || /"warn(?:ing)?"\s*:/.test(msg)) return "WARN";
  if (msg.includes("START RequestId")) return "START";
  if (msg.includes("END RequestId") || msg.includes("REPORT RequestId")) return "END";
  return "INFO";
}

export function isRequestId(value: string): boolean {
  return /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$/.test(
    value.trim(),
  );
}

/** Extract every X-Ray trace ID from log text (plain or JSON / Root=…). */
export function findTraceIds(text: string): string[] {
  if (!text) return [];
  const out: string[] = [];
  const seen = new Set<string>();
  for (const m of text.matchAll(TRACE_RE)) {
    const id = m[0];
    if (!seen.has(id)) {
      seen.add(id);
      out.push(id);
    }
  }
  return out;
}

/** Prefer server-extracted IDs; fall back to scanning the message. */
export function traceIdsForLine(message: string, fromServer?: string[] | null): string[] {
  const server = (fromServer ?? []).filter(Boolean);
  if (server.length > 0) return server;
  return findTraceIds(message);
}

/** Lambda RequestId values (from server extract or `RequestId:` scan). */
export function requestIdsForLine(message: string, fromServer?: string[] | null): string[] {
  const server = (fromServer ?? []).filter(Boolean);
  if (server.length > 0) return server;
  if (!message) return [];
  const out: string[] = [];
  const seen = new Set<string>();
  const marker = /RequestId:\s*/gi;
  let m: RegExpExecArray | null;
  while ((m = marker.exec(message))) {
    const rest = message.slice(m.index + m[0].length);
    const uuid = rest.match(
      /^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/,
    );
    if (uuid && !seen.has(uuid[0])) {
      seen.add(uuid[0]);
      out.push(uuid[0]);
    }
  }
  // Avoid treating random UUIDs in messages as request IDs when marker absent.
  if (out.length === 0 && /RequestId/i.test(message)) {
    for (const u of message.matchAll(UUID_RE)) {
      if (!seen.has(u[0])) {
        seen.add(u[0]);
        out.push(u[0]);
      }
    }
  }
  return out;
}
