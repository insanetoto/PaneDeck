import { invoke } from "@tauri-apps/api/core";
import type { DirectoryReference, EntryReference, EntrySnapshot } from "../ipc/pathReferences";

export type PaneId = "left" | "right";
export type SortField = "name" | "size" | "modifiedAt" | "kind";
export type SortDirection = "ascending" | "descending";
export type BrowseErrorCode =
  | "invalidPath"
  | "notFound"
  | "permissionDenied"
  | "notDirectory"
  | "staleReference"
  | "cancelled"
  | "io"
  | "resourceLimit";

export interface DirectoryListing {
  treeKey?: string;
  directory: DirectoryReference;
  displayPath: string;
  canGoUp: boolean;
  entries: EntrySnapshot[];
  warnings: BrowseErrorCode[];
}

export interface BrowseOptions {
  pane: PaneId;
  showHidden: boolean;
  sortField: SortField;
  sortDirection: SortDirection;
}

export interface DirectoryClient {
  holdTreeNode?(
    pane: PaneId,
    nodeId: string,
    target: { directory?: DirectoryReference; entry?: EntryReference },
  ): Promise<void>;
  readTreeNode?(nodeId: string, options: BrowseOptions): Promise<DirectoryListing>;
  releaseTreeNode?(pane: PaneId, nodeId: string): Promise<void>;
  openPath(path: string, options: BrowseOptions): Promise<DirectoryListing>;
  openChild(entry: EntryReference, options: BrowseOptions): Promise<DirectoryListing>;
  openParent(directory: DirectoryReference, options: BrowseOptions): Promise<DirectoryListing>;
  reopen(directory: DirectoryReference, options: BrowseOptions): Promise<DirectoryListing>;
}

export const tauriDirectoryClient: DirectoryClient = {
  holdTreeNode: (pane, nodeId, target) =>
    invoke("hold_tree_node", { request: { pane, nodeId, ...target } }),
  readTreeNode: (nodeId, options) => invoke("read_tree_node", { request: { nodeId, ...options } }),
  releaseTreeNode: (pane, nodeId) => invoke("release_tree_node", { request: { pane, nodeId } }),
  openPath: (path, options) => invoke("open_directory", { request: { ...options, path } }),
  openChild: (entry, options) => invoke("open_child_directory", { request: { ...options, entry } }),
  openParent: (directory, options) =>
    invoke("open_parent_directory", { request: { ...options, directory } }),
  reopen: (directory, options) =>
    invoke("reopen_directory", { request: { ...options, directory } }),
};

export function browseErrorCode(error: unknown): BrowseErrorCode {
  if (typeof error === "object" && error !== null && "code" in error) {
    const code = (error as { code: unknown }).code;
    if (typeof code === "string") return code as BrowseErrorCode;
  }
  return "io";
}
