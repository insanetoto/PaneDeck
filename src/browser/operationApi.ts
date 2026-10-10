import { listen } from "@tauri-apps/api/event";
import { isTauri } from "@tauri-apps/api/core";
import { invoke } from "@tauri-apps/api/core";
import type { DirectoryReference, EntryReference } from "../ipc/pathReferences";

export type OperationKind = "copy" | "move" | "rename" | "createDirectory" | "trash";
export type OperationItemOutcome =
  "succeeded" | "skipped" | "failed" | "cancelled" | "copiedButSourceNotRemoved";

export type OperationJobState =
  | "queued"
  | "validating"
  | "running"
  | "awaitingDecision"
  | "cancellationRequested"
  | "completed"
  | "partiallyFailed"
  | "failed"
  | "cancelled";

export type OperationPhase = "preparing" | "copying" | "verifying" | "cleaningSource" | "finished";
export interface ItemError {
  index: number | null;
  domain: string;
  code: string;
}
export interface LogStatus {
  persistenceEnabled: boolean;
  failedWrites: number;
  lastError: string | null;
}
export interface OperationLogEvent {
  schemaVersion: number;
  operationId: string;
  jobId: number | null;
  timestampMillis: number;
  kind: OperationKind;
  event: string;
  state: OperationJobState | null;
  outcomes: OperationItemOutcome[];
  errors: ItemError[];
}
export interface OperationLogSnapshot {
  events: OperationLogEvent[];
  status: LogStatus;
  recoveryStatus: LogStatus;
}
export interface OperationSnapshot {
  sequence: number;
  jobs: OperationJob[];
  logStatus: LogStatus;
  recoveryLogStatus: LogStatus;
}
export interface OperationJob {
  operationItemCount?: number;
  operationId?: string;
  operationState?: OperationJobState | null;
  operationOutcomes?: OperationItemOutcome[] | null;
  phase?: OperationPhase;
  itemErrors?: ItemError[];
  jobId: number;
  kind: OperationKind;
  state: OperationJobState;
  itemCount: number;
  completedItems: number;
  failedItems: number;
  completedUnits: number | null;
  totalUnits: number | null;
  bytesPerSecond: number | null;
  elapsedMillis: number;
  itemOutcomes: OperationItemOutcome[];
  canCancel: boolean;
  conflict?: OperationConflict;
}

export type ConflictDecision = "skip" | "replace" | "keepBoth" | "cancel";
export type ConflictKind = "fileReplacement" | "directoryMerge" | "typeMismatch";

export interface ConflictEntry {
  name: string;
  parentContext: string;
  kind: string;
  byteLen: number;
  modifiedMillis: number | null;
}

export interface OperationConflict {
  kind: ConflictKind;
  source: ConflictEntry;
  target: ConflictEntry;
  replaceAllowed: boolean;
}

export interface OperationClient {
  watch?(receive: (snapshot: OperationSnapshot) => void): Promise<() => void>;
  getLog?(): Promise<OperationLogSnapshot>;
  validateTransfer?(
    kind: "copy" | "move",
    entries: EntryReference[],
    destination:
      | { kind: "entry"; entry: EntryReference }
      | { kind: "directory"; directory: DirectoryReference },
  ): Promise<void>;
  transfer(
    kind: "copy" | "move",
    entries: EntryReference[],
    destination: DirectoryReference,
  ): Promise<OperationJob>;
  transferToEntry(
    kind: "copy" | "move",
    entries: EntryReference[],
    destination: EntryReference,
  ): Promise<OperationJob>;
  rename(entry: EntryReference, newName: string): Promise<OperationJob>;
  createDirectory(directory: DirectoryReference, name: string): Promise<OperationJob>;
  trash(entries: EntryReference[]): Promise<OperationJob>;
  listJobs(): Promise<OperationJob[]>;
  cancel(jobId: number): Promise<OperationJob>;
  resolveConflict(
    jobId: number,
    decision: ConflictDecision,
    applyToRemaining: boolean,
  ): Promise<OperationJob>;
}

export interface OperationWatchTransport {
  listen(receive: (token: string, snapshot: OperationSnapshot) => void): Promise<() => void>;
  start(token: string): Promise<[number, OperationSnapshot]>;
  stop(id: number): Promise<void>;
}
/** Listen before snapshot; a buffered newer snapshot wins over a delayed IPC response. */
export async function watchOperations(
  transport: OperationWatchTransport,
  receive: (snapshot: OperationSnapshot) => void,
) {
  const token = crypto.randomUUID();
  let ready = false,
    disposed = false,
    latest = -1,
    buffered: OperationSnapshot | null = null;
  const accept = (snapshot: OperationSnapshot) => {
    if (disposed || snapshot.sequence <= latest) return;
    latest = snapshot.sequence;
    receive(snapshot);
  };
  const stopListen = await transport.listen((scope, snapshot) => {
    if (scope !== token || disposed) return;
    if (!ready) {
      if (!buffered || snapshot.sequence > buffered.sequence) buffered = snapshot;
    } else accept(snapshot);
  });
  let id: number;
  try {
    const result = await transport.start(token);
    id = result[0];
    ready = true;
    const snapshot = result[1];
    const pending = buffered as OperationSnapshot | null;
    accept(pending && pending.sequence > snapshot.sequence ? pending : snapshot);
  } catch (error) {
    disposed = true;
    stopListen();
    throw error;
  }
  return () => {
    if (disposed) return;
    disposed = true;
    stopListen();
    void transport.stop(id);
  };
}
export const tauriOperationClient: OperationClient = {
  watch: async (receive) => {
    if (!isTauri()) {
      receive({
        sequence: 0,
        jobs: [],
        logStatus: { persistenceEnabled: false, failedWrites: 0, lastError: null },
        recoveryLogStatus: { persistenceEnabled: false, failedWrites: 0, lastError: null },
      });
      return () => {};
    }
    return watchOperations(
      {
        listen: async (callback) =>
          listen<[string, OperationSnapshot]>("operation-state", (event) =>
            callback(...event.payload),
          ),
        start: (token) => invoke("start_operation_watch", { token }),
        stop: (watchId) => invoke("stop_operation_watch", { watchId }),
      },
      receive,
    );
  },
  getLog: () => invoke("operation_log"),
  validateTransfer: (kind, entries, destination) =>
    invoke("validate_transfer", { request: { kind, entries, destination } }),
  transfer: (kind, entries, destination) =>
    invoke("execute_transfer", {
      request: { kind, entries, destination: { kind: "directory", directory: destination } },
    }),
  transferToEntry: (kind, entries, destination) =>
    invoke("execute_transfer", {
      request: { kind, entries, destination: { kind: "entry", entry: destination } },
    }),
  rename: (entry, newName) => invoke("rename_entry", { request: { entry, newName } }),
  createDirectory: (directory, name) =>
    invoke("create_directory", { request: { directory, name } }),
  trash: (entries) => invoke("trash_entries", { request: { entries } }),
  listJobs: () => invoke("list_operation_jobs"),
  cancel: (jobId) => invoke("cancel_operation_job", { request: { jobId } }),
  resolveConflict: (jobId, decision, applyToRemaining) =>
    invoke("resolve_operation_conflict", { request: { jobId, decision, applyToRemaining } }),
};
