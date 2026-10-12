import { afterEach, describe, expect, it, vi } from "vitest";
import type { EntrySnapshot } from "../ipc/pathReferences";
import type { BrowseOptions, DirectoryClient, DirectoryListing } from "./directoryApi";
import { DirectoryTree, normalizeTreeSelection, selectionAfterCollapse } from "./directoryTree";
import { entryKey } from "./sortEntries";
const options: BrowseOptions = {
  pane: "left",
  showHidden: false,
  sortField: "name",
  sortDirection: "ascending",
};
const entry = (
  name: string,
  session = "1",
  kind: EntrySnapshot["kind"] = "directory",
): EntrySnapshot => ({
  reference: { sessionId: session, entryId: name },
  treeKey: name,
  displayName: name,
  displayNameIsLossy: false,
  kind,
  byteLen: null,
  modifiedAtUnixMillis: null,
  isHidden: false,
  isReadOnly: false,
  symlinkTarget: kind === "symlink" ? "directory" : null,
});
const listing = (session: string, entries: EntrySnapshot[]): DirectoryListing => ({
  directory: { sessionId: session },
  displayPath: "/test",
  canGoUp: true,
  entries,
  warnings: [],
});
function setup() {
  const targets = new Map<string, string>();
  const client: DirectoryClient = {
    openPath: vi.fn(),
    openChild: vi.fn(),
    openParent: vi.fn(),
    reopen: vi.fn(),
    holdTreeNode: vi.fn(async (_pane, id, target) => {
      targets.set(id, target.entry?.entryId ?? "root");
    }),
    releaseTreeNode: vi.fn(async () => {}),
    readTreeNode: vi.fn(async (id) => {
      const name = targets.get(id);
      return name === "A"
        ? listing("2", [entry("B", "2"), entry("z", "2", "file")])
        : name === "B"
          ? listing("3", [entry("C", "3")])
          : listing("4", [entry("file", "4", "file")]);
    }),
  };
  const root = listing("1", [entry("Z"), entry("A"), entry("link", "1", "symlink")]);
  const tree = new DirectoryTree(client, root, options);
  tree.start();
  return { tree, root, client };
}
afterEach(() => vi.useRealTimers());

describe("directory tree model", () => {
  it("reads only expanded folders, preserves three levels and per-parent sorting", async () => {
    const { tree, root, client } = setup();
    expect(client.readTreeNode).not.toHaveBeenCalled();
    await tree.expand("A");
    await tree.expand("B");
    await tree.expand("C");
    await tree.expand("link");
    expect(tree.snapshot().map((row) => [row.entry.displayName, row.level])).toEqual([
      ["A", 1],
      ["B", 2],
      ["C", 3],
      ["file", 4],
      ["z", 2],
      ["link", 1],
      ["Z", 1],
    ]);
    expect(client.readTreeNode).toHaveBeenCalledTimes(3);
    expect(root.displayPath).toBe("/test");
    expect(client.openChild).not.toHaveBeenCalled();
    tree.update(root, { ...options, sortDirection: "descending" });
    expect(tree.snapshot().map((row) => row.entry.displayName)).toEqual([
      "Z",
      "link",
      "A",
      "z",
      "B",
      "C",
      "file",
    ]);
    tree.dispose();
  });
  it("normalizes parent-child selection and releases collapsed descendants", async () => {
    const { tree, client } = setup();
    await tree.expand("A");
    await tree.expand("B");
    const rows = tree.snapshot();
    expect(
      normalizeTreeSelection(
        rows,
        new Set(
          rows.filter((row) => ["A", "B", "C"].includes(row.key)).map((row) => entryKey(row.entry)),
        ),
      ).map((e) => e.displayName),
    ).toEqual(["A"]);
    const collapse = selectionAfterCollapse(
      rows,
      new Set(rows.map((row) => entryKey(row.entry))),
      entryKey(rows.find((row) => row.key === "C")!.entry),
      "A",
    );
    expect(collapse.focused).toBe(entryKey(rows[0].entry));
    expect(collapse.selected.has(entryKey(rows.find((row) => row.key === "C")!.entry))).toBe(false);
    tree.collapse("A");
    expect(tree.snapshot().some((row) => row.key === "B")).toBe(false);
    expect(client.releaseTreeNode).toHaveBeenCalledTimes(2);
    tree.dispose();
  });
  it("refreshes cached branch ordering when sorting or root snapshots change", async () => {
    const { tree, root } = setup();
    await tree.expand("A");
    expect(
      tree
        .snapshot()
        .map((row) => row.entry.displayName)
        .slice(0, 3),
    ).toEqual(["A", "B", "z"]);
    tree.update(root, { ...options, sortDirection: "descending" });
    await vi.waitFor(() =>
      expect(tree.snapshot().find((row) => row.parentKey === "A")?.entry.displayName).toBe("z"),
    );
    tree.update(listing("new", [entry("fresh", "new", "file")]), options);
    expect(tree.snapshot().map((row) => row.entry.displayName)).toEqual(["fresh"]);
    tree.dispose();
  });

  it("discards a result delivered after collapse and keeps pane states independent", async () => {
    const { tree, client, root } = setup();
    let finish!: (listing: DirectoryListing) => void;
    client.readTreeNode = vi.fn(
      () =>
        new Promise<DirectoryListing>((resolve) => {
          finish = resolve;
        }),
    );
    const expanding = tree.expand("A");
    await vi.waitFor(() => expect(client.readTreeNode).toHaveBeenCalledTimes(1));
    tree.collapse("A");
    finish(listing("2", [entry("late", "2")]));
    await expanding;
    expect(tree.snapshot().some((row) => row.key === "late")).toBe(false);
    const other = new DirectoryTree(client, root, { ...options, pane: "right" });
    other.start();
    expect(other.snapshot().every((row) => !row.expanded)).toBe(true);
    tree.dispose();
    other.dispose();
  });
  it("isolates read failures and retries without clearing root siblings", async () => {
    const { tree, client } = setup();
    vi.mocked(client.readTreeNode!).mockRejectedValueOnce({ code: "permissionDenied" });
    await tree.expand("A");
    expect(tree.snapshot().find((row) => row.key === "A")?.error).toBe("permissionDenied");
    expect(tree.snapshot().some((row) => row.key === "Z")).toBe(true);
    tree.retry("A");
    await vi.waitFor(() => expect(tree.snapshot().find((row) => row.key === "B")).toBeDefined());
    tree.dispose();
  });
});

describe("expanded branch synchronization", () => {
  it("refreshes the affected level, clears removed descendants and retains expanded roots across rescans", async () => {
    vi.useFakeTimers();
    const { tree, root, client } = setup();
    const signals = new Map<string, () => void>();
    const stops = new Map<string, ReturnType<typeof vi.fn>>();
    tree.setMonitor(
      {
        start: vi.fn(async (_pane, directory, signal) => {
          signals.set(directory.sessionId, () => signal("changed"));
          const stop = vi.fn();
          stops.set(directory.sessionId, stop);
          return stop;
        }),
      },
      vi.fn(async () => {}),
      vi.fn(),
    );
    await tree.expand("A");
    await tree.expand("B");
    await Promise.resolve();
    vi.mocked(client.readTreeNode!).mockResolvedValueOnce(
      listing("5", [entry("renamed", "5", "file")]),
    );
    signals.get("2")!();
    await vi.advanceTimersByTimeAsync(150);
    expect(tree.snapshot().find((row) => row.key === "renamed")?.level).toBe(2);
    expect(tree.snapshot().some((row) => row.key === "C")).toBe(false);
    expect(stops.get("3")).toHaveBeenCalledOnce();
    tree.update(
      listing(
        "6",
        root.entries.map((item) => ({ ...item, reference: { ...item.reference, sessionId: "6" } })),
      ),
      options,
    );
    expect(tree.snapshot().find((row) => row.key === "A")?.expanded).toBe(true);
    expect(tree.snapshot().some((row) => row.key === "renamed")).toBe(true);
    vi.mocked(client.readTreeNode!).mockRejectedValueOnce({ code: "notFound" });
    signals.get("2")!();
    await vi.advanceTimersByTimeAsync(150);
    expect(tree.snapshot().find((row) => row.key === "A")?.error).toBe("notFound");
    expect(tree.snapshot().some((row) => row.key === "Z")).toBe(true);
    expect(tree.snapshot().some((row) => row.key === "renamed")).toBe(false);
    tree.dispose();
  });
});

describe("v2 tree resource audit", () => {
  it("releases holds, watchers and timers over 200 expansion and navigation cycles", async () => {
    vi.useFakeTimers();
    const holds = new Set<string>(),
      watches = new Set<string>();
    let maximumHolds = 0,
      maximumWatches = 0;
    const client: DirectoryClient = {
      openPath: vi.fn(),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn(),
      holdTreeNode: async (_pane, id) => {
        holds.add(id);
        maximumHolds = Math.max(maximumHolds, holds.size);
      },
      releaseTreeNode: async (_pane, id) => {
        holds.delete(id);
      },
      readTreeNode: async (id) => listing(id, [entry("child", id)]),
    };
    for (let cycle = 0; cycle < 200; cycle++) {
      const tree = new DirectoryTree(client, listing(`root-${cycle}`, [entry("folder")]), options);
      tree.start();
      tree.setMonitor(
        {
          start: async (_pane, directory, signal) => {
            watches.add(directory.sessionId);
            maximumWatches = Math.max(maximumWatches, watches.size);
            signal("changed");
            return () => {
              watches.delete(directory.sessionId);
            };
          },
        },
        async () => {},
        vi.fn(),
      );
      await tree.expand("folder");
      tree.collapse("folder");
      tree.dispose();
      await Promise.resolve();
      expect(holds.size).toBe(0);
      expect(watches.size).toBe(0);
      expect(vi.getTimerCount()).toBe(0);
    }
    expect(maximumHolds).toBeLessThanOrEqual(2);
    expect(maximumWatches).toBeLessThanOrEqual(2);
  });
});
