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

export interface OperationJob {
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

export const tauriOperationClient: OperationClient = {
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
