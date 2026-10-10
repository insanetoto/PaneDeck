import type { DirectoryMonitor } from "./directoryMonitor";
import { TreeWatchCoordinator, type WatchTarget } from "./treeWatchCoordinator";
import type { EntrySnapshot } from "../ipc/pathReferences";
import {
  browseErrorCode,
  type BrowseErrorCode,
  type BrowseOptions,
  type DirectoryClient,
  type DirectoryListing,
} from "./directoryApi";
import { entryKey, sortEntries } from "./sortEntries";

export interface TreeRow {
  entry: EntrySnapshot;
  key: string;
  parentKey: string | null;
  level: number;
  position: number;
  siblings: number;
  expanded: boolean;
  loading: boolean;
  error: BrowseErrorCode | null;
  watchError?: "watchFailed" | "resourceLimit" | null;
}
interface Node {
  id: string;
  key: string;
  parentKey: string | null;
  listing: DirectoryListing | null;
  loading: boolean;
  error: BrowseErrorCode | null;
  generation: number;
  held: boolean;
  watchError: "watchFailed" | "resourceLimit" | null;
}
let nextNode = 0;
export const treeKey = (entry: EntrySnapshot) => entry.treeKey ?? entryKey(entry);

/** Only expanded branches are read. Native paths never enter this model. */
export class DirectoryTree {
  private nodes = new Map<string, Node>();
  private listeners = new Set<() => void>();
  private rows: TreeRow[] = [];
  private active = false;
  private rootHold: { id: string; ready: Promise<void> } | null = null;
  private options: BrowseOptions;
  private coordinator: TreeWatchCoordinator | null = null;
  private rootRescan: (() => Promise<void>) | null = null;
  private rootWatchFailed: ((reason: "watchFailed" | "resourceLimit") => void) | null = null;
  private rootMissing = false;
  constructor(
    private client: DirectoryClient,
    private root: DirectoryListing | null,
    options: BrowseOptions,
    readonly rootScope?: string,
  ) {
    this.options = options;
    this.rebuild();
  }
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  snapshot = () => this.rows;
  private release(id: string) {
    void this.client.releaseTreeNode?.(this.options.pane, id).catch(() => undefined);
  }
  setMonitor(
    monitor: DirectoryMonitor | undefined,
    rescan: () => Promise<void>,
    failed: (reason: "watchFailed" | "resourceLimit") => void,
  ) {
    this.coordinator?.dispose();
    this.coordinator = monitor ? new TreeWatchCoordinator(monitor, this.options.pane) : null;
    this.rootRescan = rescan;
    this.rootWatchFailed = failed;
    this.syncWatches();
  }
  private syncWatches() {
    if (!this.active || !this.coordinator) return;
    const targets: WatchTarget[] = [];
    if (this.root && !this.rootMissing && this.rootRescan)
      targets.push({
        key: "root",
        directory: this.root.directory,
        rescan: this.rootRescan,
        failed: (reason) => this.rootWatchFailed?.(reason),
      });
    for (const node of this.nodes.values())
      if (node.listing && node.held)
        targets.push({
          key: node.id,
          directory: node.listing.directory,
          rescan: async () => {
            if (this.isLive(node)) await this.read(node);
          },
          failed: (reason) => {
            if (!this.isLive(node) || node.watchError === reason) return;
            node.watchError = reason;
            this.rebuild();
          },
        });
    this.coordinator.sync(targets);
  }
  invalidateRoot() {
    this.rootMissing = true;
    for (const node of this.nodes.values()) this.release(node.id);
    this.nodes.clear();
    this.rebuild();
  }
  refreshBranches() {
    this.coordinator?.retry();
    for (const node of this.nodes.values()) {
      node.watchError = null;
      if (node.held && !node.loading) void this.read(node);
    }
    this.syncWatches();
  }
  start() {
    this.active = true;
    this.bindRoot();
  }
  dispose() {
    this.active = false;
    this.coordinator?.dispose();
    this.coordinator = null;
    for (const node of this.nodes.values()) this.release(node.id);
    this.nodes.clear();
    if (this.rootHold) this.release(this.rootHold.id);
    this.rootHold = null;
  }
  private bindRoot() {
    if (!this.root || !this.client.holdTreeNode || !this.active) return;
    const previous = this.rootHold;
    const id = `tree-${++nextNode}`;
    const ready = this.client.holdTreeNode(this.options.pane, id, {
      directory: this.root.directory,
    });
    const hold = { id, ready };
    this.rootHold = hold;
    void ready
      .then(() => {
        if (!this.active || this.rootHold !== hold) this.release(id);
      })
      .catch(() => undefined)
      .finally(() => {
        if (previous) this.release(previous.id);
      });
  }
  update(root: DirectoryListing | null, options: BrowseOptions) {
    const changedRoot = this.root !== root;
    const changedOptions =
      this.options.showHidden !== options.showHidden ||
      this.options.sortField !== options.sortField ||
      this.options.sortDirection !== options.sortDirection;
    this.root = root;
    this.options = options;
    if (changedRoot) {
      this.rootMissing = false;
      this.bindRoot();
    }
    this.rebuild();
    if (changedOptions) for (const node of this.nodes.values()) if (node.held) void this.read(node);
  }
  private rebuild() {
    const rows: TreeRow[] = [];
    const live = new Set<string>();
    const visit = (entries: EntrySnapshot[], parentKey: string | null, level: number) => {
      const siblings = sortEntries(entries, this.options.sortField, this.options.sortDirection);
      siblings.forEach((entry, index) => {
        const key = treeKey(entry);
        live.add(key);
        const node = this.nodes.get(key);
        rows.push({
          entry,
          key,
          parentKey,
          level,
          position: index + 1,
          siblings: siblings.length,
          expanded: !!node,
          loading: node?.loading ?? false,
          error: node?.error ?? null,
          watchError: node?.watchError ?? null,
        });
        if (node?.listing) visit(node.listing.entries, key, level + 1);
      });
    };
    visit(this.rootMissing ? [] : (this.root?.entries ?? []), null, 1);
    for (const [key, node] of this.nodes)
      if (!live.has(key)) {
        this.nodes.delete(key);
        this.release(node.id);
      }
    this.rows = rows;
    this.syncWatches();
    for (const listener of this.listeners) listener();
  }
  collapse(key: string) {
    const remove = (target: string) => {
      for (const child of [...this.nodes.values()])
        if (child.parentKey === target) remove(child.key);
      const node = this.nodes.get(target);
      if (node) {
        this.nodes.delete(target);
        this.release(node.id);
      }
    };
    remove(key);
    this.rebuild();
  }
  async expand(key: string) {
    const row = this.rows.find((row) => row.key === key);
    if (!row || row.entry.kind !== "directory" || this.nodes.has(key) || !this.active) return;
    const node: Node = {
      id: `tree-${++nextNode}`,
      key,
      parentKey: row.parentKey,
      listing: null,
      loading: true,
      error: null,
      generation: 0,
      held: false,
      watchError: null,
    };
    this.nodes.set(key, node);
    this.rebuild();
    try {
      if (!this.client.holdTreeNode || !this.client.readTreeNode) throw { code: "io" };
      await this.rootHold?.ready;
      if (!this.isLive(node)) return;
      await this.client.holdTreeNode(this.options.pane, node.id, { entry: row.entry.reference });
      if (!this.isLive(node)) {
        this.release(node.id);
        return;
      }
      node.held = true;
      await this.read(node);
    } catch (error) {
      if (this.isLive(node)) {
        node.loading = false;
        node.error = browseErrorCode(error);
        this.rebuild();
      }
    }
  }
  private isLive(node: Node) {
    return this.active && this.nodes.get(node.key) === node;
  }
  private async read(node: Node) {
    if (!this.client.readTreeNode || !this.isLive(node)) return;
    const generation = ++node.generation;
    const live = () => this.isLive(node) && node.generation === generation;
    node.loading = true;
    node.error = null;
    this.rebuild();
    try {
      const listing = await this.client.readTreeNode(node.id, this.options);
      if (!live()) return;
      node.listing = listing;
    } catch (error) {
      if (!live()) return;
      node.error = browseErrorCode(error);
      // Keep the failing branch isolated; stale children cannot be operated on.
      node.listing = null;
    } finally {
      if (live()) {
        node.loading = false;
        this.rebuild();
      }
    }
  }
  retry(key: string) {
    const node = this.nodes.get(key);
    if (!node || node.loading) return;
    this.coordinator?.retry(node.id);
    node.watchError = null;
    if (node.held) void this.read(node);
    else {
      this.collapse(key);
      void this.expand(key);
    }
  }
}

/** Selected descendants remain highlighted but are omitted from batch commands. */
export function normalizeTreeSelection(
  rows: readonly TreeRow[],
  selected: ReadonlySet<string>,
): EntrySnapshot[] {
  const byKey = new Map(rows.map((row) => [row.key, row]));
  return rows
    .filter((row) => {
      if (!selected.has(entryKey(row.entry))) return false;
      let parent = row.parentKey;
      while (parent) {
        const ancestor = byKey.get(parent);
        if (ancestor && selected.has(entryKey(ancestor.entry))) return false;
        parent = ancestor?.parentKey ?? null;
      }
      return true;
    })
    .map((row) => row.entry);
}

export function selectionAfterCollapse(
  rows: readonly TreeRow[],
  selected: ReadonlySet<string>,
  focused: string | null,
  key: string,
) {
  const parent = rows.find((row) => row.key === key);
  if (!parent) return { selected: new Set(selected), focused };
  const descendants = new Set<string>();
  const index = rows.indexOf(parent);
  for (let i = index + 1; i < rows.length && rows[i].level > parent.level; i++)
    descendants.add(entryKey(rows[i].entry));
  return {
    selected: new Set([...selected].filter((id) => !descendants.has(id))),
    focused: focused && descendants.has(focused) ? entryKey(parent.entry) : focused,
  };
}
