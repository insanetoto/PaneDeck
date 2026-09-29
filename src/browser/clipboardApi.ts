import { invoke } from "@tauri-apps/api/core";
import type { DirectoryReference, EntryReference } from "../ipc/pathReferences";
import type { PaneId } from "./directoryApi";
import type { OperationJob } from "./operationApi";

export type ClipboardIntent = "copy" | "cut";

export interface ClipboardState {
  intent: ClipboardIntent;
  sourcePane: PaneId | null;
  itemCount: number;
  cutEntries: EntryReference[];
  systemSynced: boolean;
}

export interface PasteDraft {
  draftId: number;
  intent: ClipboardIntent;
  itemCount: number;
  targetPane: PaneId;
  destinationDisplayPath: string;
}

export interface ClipboardClient {
  set(pane: PaneId, intent: ClipboardIntent, entries: EntryReference[]): Promise<ClipboardState>;
  prepare(targetPane: PaneId, destination: DirectoryReference): Promise<PasteDraft>;
  confirm(draftId: number): Promise<OperationJob>;
  cancel(draftId: number): Promise<void>;
}

export const tauriClipboardClient: ClipboardClient = {
  set: (pane, intent, entries) =>
    invoke("set_file_clipboard", { request: { pane, intent, entries } }),
  prepare: (targetPane, destination) =>
    invoke("prepare_file_paste", { request: { targetPane, destination } }),
  confirm: (draftId) => invoke("confirm_file_paste", { request: { draftId } }),
  cancel: (draftId) => invoke("cancel_file_paste", { request: { draftId } }),
};
