import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, vi } from "vitest";
import { I18nProvider } from "./i18n/I18nProvider";
import { VirtualFileList } from "./browser/VirtualFileList";
import { DirectoryTree } from "./browser/directoryTree";
import type { DirectoryClient, DirectoryListing } from "./browser/directoryApi";
import { describe, expect, it } from "vitest";
import type { EntrySnapshot } from "./ipc/pathReferences";
import { calculateVisibleRange, sortEntries } from "./browser/VirtualFileList";

function fixedEntries(count: number): EntrySnapshot[] {
  return Array.from({ length: count }, (_, index) => ({
    reference: { sessionId: "benchmark", entryId: String(index) },
    displayName: `item-${String(count - index).padStart(5, "0")}`,
    displayNameIsLossy: false,
    kind: index % 7 === 0 ? "directory" : "file",
    byteLen: String(index * 97),
    modifiedAtUnixMillis: String(1_700_000_000_000 + index),
    isHidden: false,
    isReadOnly: false,
    symlinkTarget: null,
  }));
}

describe("repeatable UI performance budget", () => {
  afterEach(cleanup);
  it("sorts 10,000 fixed entries inside the first-visible budget", () => {
    const fixture = fixedEntries(10_000);
    const started = performance.now();
    const sorted = sortEntries(fixture, "name", "ascending");
    const elapsed = performance.now() - started;

    expect(sorted).toHaveLength(10_000);
    console.info(`v2 flat sort: ${elapsed.toFixed(2)} ms`);
    expect(elapsed).toBeLessThan(800);
  });

  it("keeps every virtual-scroll window calculation below one 60 FPS frame", () => {
    let slowest = 0;
    for (let frame = 0; frame < 600; frame += 1) {
      const started = performance.now();
      const range = calculateVisibleRange(frame * 48, 720, 10_000);
      slowest = Math.max(slowest, performance.now() - started);
      expect(range.count).toBeLessThan(50);
    }
    console.info(`v2 scroll range worst / 600: ${slowest.toFixed(3)} ms`);
    expect(slowest).toBeLessThan(16);
  });
  it("flattens 10,000 hierarchical rows and keeps rendered DOM bounded while scrolling", async () => {
    const options = {
      pane: "left" as const,
      showHidden: false,
      sortField: "name" as const,
      sortDirection: "ascending" as const,
    };
    const listing = (sessionId: string, entries: EntrySnapshot[]): DirectoryListing => ({
      directory: { sessionId },
      displayPath: "/benchmark",
      canGoUp: true,
      entries,
      warnings: [],
    });
    const roots = fixedEntries(100).map((entry, index) => ({
      ...entry,
      kind: "directory" as const,
      treeKey: `root-${index}`,
    }));
    const client: DirectoryClient = {
      openPath: vi.fn(),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn(),
      holdTreeNode: async () => {},
      releaseTreeNode: async () => {},
      readTreeNode: async (id) =>
        listing(
          id,
          fixedEntries(99).map((entry) => ({
            ...entry,
            kind: "file" as const,
            reference: { ...entry.reference, sessionId: id },
            treeKey: `${id}-${entry.reference.entryId}`,
          })),
        ),
    };
    const tree = new DirectoryTree(client, listing("roots", roots), options);
    tree.start();
    const started = performance.now();
    for (const entry of roots) await tree.expand(entry.treeKey);
    const rows = tree.snapshot();
    console.info(
      `v2 100 branches / 10,000 visible tree rows: ${(performance.now() - started).toFixed(2)} ms`,
    );
    expect(rows).toHaveLength(10_000);
    const flattenStarted = performance.now();
    tree.update(listing("roots", roots), options);
    const flattenElapsed = performance.now() - flattenStarted;
    console.info(`v2 10,000 tree rows rebuild: ${flattenElapsed.toFixed(2)} ms`);
    expect(flattenElapsed).toBeLessThan(800);
    const view = render(
      <I18nProvider>
        <VirtualFileList
          entries={rows.map((row) => row.entry)}
          treeRows={rows}
          sortField="name"
          sortDirection="ascending"
          onOpen={vi.fn()}
          onSort={vi.fn()}
        />
      </I18nProvider>,
    );
    const viewport = view.container.querySelector<HTMLElement>(".file-list-viewport")!;
    for (const offset of [0, 32_000, 160_000, 310_000]) {
      viewport.scrollTop = offset;
      fireEvent.scroll(viewport);
      expect(screen.getAllByRole("row").length).toBeLessThan(40);
      expect(screen.getByRole("treegrid")).toHaveAttribute("aria-rowcount", "10001");
    }
    tree.dispose();
  });
});
