import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AwsProfile,
  CdkOutInspect,
  Connection,
  LogLine,
  LogsCursor,
  ResourceGraphPayload,
  SampleGraph,
  SearchLogsResult,
} from "./types";

export interface SearchLogsArgs {
  connection: Connection;
  cdkOut?: string;
  constructPath: string;
  back?: string;
  start?: string;
  end?: string;
  filter?: string;
  limit?: number;
  /** Continue a previous search ("load newer"). */
  cursor?: LogsCursor;
}

function toLogsInput(args: SearchLogsArgs) {
  return {
    connection: args.connection,
    cdkOut: args.cdkOut ?? null,
    constructPath: args.constructPath,
    back: args.back ?? null,
    start: args.start ?? null,
    end: args.end ?? null,
    filter: args.filter ?? null,
    limit: args.limit ?? 100,
    cursor: args.cursor ?? null,
  };
}

export const api = {
  getStatus: (connection: Connection) =>
    invoke<{ version: string; profile: string; region: string }>("get_status", {
      connection,
    }),
  getGraph: (cdkOut?: string) =>
    invoke<ResourceGraphPayload>("get_graph", { cdkOut: cdkOut ?? null }),
  searchLogs: (args: SearchLogsArgs) =>
    invoke<SearchLogsResult>("search_logs", { input: toLogsInput(args) }),
  fetchTrace: (connection: Connection, id: string, cdkOut?: string) =>
    invoke<{ trace: unknown; suggestedLogWindow: unknown }>("fetch_trace", {
      input: {
        connection,
        id,
        cdkOut: cdkOut ?? null,
      },
    }),
  resolveRequestTrace: (
    connection: Connection,
    requestId: string,
    aroundMs?: number,
    back?: string,
  ) =>
    invoke<{ trace: unknown; suggestedLogWindow: unknown }>("resolve_request_trace", {
      input: {
        connection,
        requestId,
        aroundMs: aroundMs ?? null,
        back: back ?? null,
      },
    }),
  verifyLogGroups: (connection: Connection, cdkOut?: string) =>
    invoke<{ missing: string[]; checked: string[] }>("verify_log_groups", {
      input: { connection, cdkOut: cdkOut ?? null },
    }),
  runSynth: (projectDir: string) =>
    invoke<{ outdir: string; stale: boolean }>("run_synth", { projectDir }),
  testConnection: (connection: Connection) =>
    invoke<{ ok: boolean; profile: string; region: string }>("test_connection", {
      connection,
    }),
  runInsights: (args: {
    connection: Connection;
    cdkOut?: string;
    constructPath: string;
    query: string;
    back?: string;
    start?: string;
    end?: string;
    limit?: number;
  }) =>
    invoke<Record<string, string>[]>("run_insights", {
      input: {
        connection: args.connection,
        cdkOut: args.cdkOut ?? null,
        constructPath: args.constructPath,
        query: args.query,
        back: args.back ?? null,
        start: args.start ?? null,
        end: args.end ?? null,
        limit: args.limit ?? 50,
      },
    }),
  listTraceSummaries: (args: {
    connection: Connection;
    back?: string;
    start?: string;
    end?: string;
    filter?: string;
    service?: string;
    limit?: number;
  }) =>
    invoke<
      { id: string; duration_secs?: number; has_fault: boolean; has_error: boolean }[]
    >("list_trace_summaries", {
      input: {
        connection: args.connection,
        back: args.back ?? null,
        start: args.start ?? null,
        end: args.end ?? null,
        filter: args.filter ?? null,
        service: args.service ?? null,
        limit: args.limit ?? 25,
      },
    }),
  revealInFinder: (path: string) => invoke<void>("reveal_in_finder", { path }),
  openExternal: (url: string) => invoke<void>("open_external", { url }),
  openTerminalWith: (command: string) => invoke<void>("open_terminal_with", { command }),
  startTail: (args: SearchLogsArgs, intervalSecs = 2) =>
    invoke<void>("start_tail", { input: toLogsInput(args), intervalSecs }),
  stopTail: () => invoke<void>("stop_tail"),
  inspectCdkOut: (projectDir?: string, cdkOut?: string) =>
    invoke<CdkOutInspect>("inspect_cdk_out", {
      projectDir: projectDir ?? null,
      cdkOut: cdkOut ?? null,
    }),
  loadSampleGraph: () => invoke<SampleGraph>("load_sample_graph"),
  listAwsProfiles: () => invoke<AwsProfile[]>("list_aws_profiles"),
  pickFolder: () =>
    open({ directory: true, multiple: false, title: "Open CDK project" }),
};

/// Subscribe to batched log batches (FR-21). Returns an unlisten fn.
export function onLogBatch(cb: (batch: LogLine[]) => void): Promise<UnlistenFn> {
  return listen<LogLine[]>("log-batch", (e) => cb(e.payload));
}

export function onLogError(cb: (msg: string) => void): Promise<UnlistenFn> {
  return listen<string>("log-error", (e) => cb(e.payload));
}
