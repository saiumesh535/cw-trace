// Shared shapes mirror the Rust Serialize structs (camelCase).

export type Confidence = "resolved" | "inferred";

export interface ResourceNode {
  construct_path: string;
  cfn_type: string;
  kind: string;
  logical_id?: string;
  fqn?: string;
  log_group?: string;
  confidence?: Confidence;
  function_name?: string;
  runtime?: string;
  /** Physical name for data-plane resources (table, queue, …). */
  resource_name?: string;
}

export interface ResourceEdge {
  from: string;
  to: string;
  via: string;
}

export interface ResourceGraphPayload {
  nodes: ResourceNode[];
  edges: ResourceEdge[];
}

/** Per-construct activity for the live construct map. */
export interface NodeHeat {
  lastPulseMs: number;
  severity: "ERROR" | "WARN" | "START" | "END" | "INFO";
  errCount: number;
  lastError?: string;
}

export interface LogLine {
  timestampMs: number;
  timestamp: string;
  logGroup: string;
  logStream: string;
  message: string;
  traceIds: string[];
  lambdaRequestIds: string[];
  apigwRequestIds: string[];
}

export interface XRaySubsegment {
  id: string;
  name: string;
  start_ms: number;
  end_ms: number;
  origin?: string;
  namespace?: string;
  aws_operation?: string;
  http_status?: number;
  error?: boolean;
  fault?: boolean;
  throttle?: boolean;
  cause?: string;
  subsegments?: XRaySubsegment[];
}

export interface XRaySegment {
  id: string;
  name: string;
  start_ms: number;
  end_ms: number;
  origin?: string;
  error?: boolean;
  fault?: boolean;
  throttle?: boolean;
  http_status?: number;
  cause?: string;
  subsegments?: XRaySubsegment[];
  document: unknown;
}

export interface XRayTrace {
  id: string;
  duration_secs?: number;
  segments: XRaySegment[];
}

export interface Connection {
  profile?: string;
  region?: string;
}

export interface CdkOutInspect {
  cdkJsonFound: boolean;
  appCommand?: string;
  outdir: string;
  stacks: string[];
  stageHints: string[];
  nodeCount: number;
  logGroupCount: number;
  outMtimeMs?: number;
  srcMtimeMs?: number;
  stale: boolean;
  error?: string;
}

export interface SampleGraph {
  nodes: ResourceNode[];
  edges?: ResourceEdge[];
  cdkOut: string;
}

export interface GroupCursor {
  logGroup: string;
  nextToken?: string;
  done: boolean;
}

export interface LogsCursor {
  groups: GroupCursor[];
}

export interface SearchLogsResult {
  lines: LogLine[];
  missing: string[];
  nextCursor?: LogsCursor;
}

export interface AwsProfile {
  name: string;
  region?: string;
  source: string;
}

// ---------- local workspace (projects / stages / pins) ----------

export interface Project {
  id: string;
  name: string;
  path: string;
  createdMs: number;
}

export interface Stage {
  id: string;
  projectId: string;
  name: string;
  cdkOut: string;
  profile?: string;
  region?: string;
  stackFilter?: string;
  pinned: boolean;
  lastOpenedMs?: number;
  lastError?: string;
}

export type UiTheme = "system" | "light" | "dark";

export interface Settings {
  defaultProfile?: string;
  defaultRegion?: string;
  tailIntervalSecs: number;
  bufferLimit: number;
  /** Webview zoom factor (0.5–2.0). Applied to <body> via CSS `zoom`. */
  uiZoom: number;
  /** Appearance: follow OS, or force light/dark. */
  uiTheme: UiTheme;
}
