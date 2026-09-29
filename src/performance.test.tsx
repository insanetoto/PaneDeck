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
  it("sorts 10,000 fixed entries inside the first-visible budget", () => {
    const fixture = fixedEntries(10_000);
    const started = performance.now();
    const sorted = sortEntries(fixture, "name", "ascending");
    const elapsed = performance.now() - started;

    expect(sorted).toHaveLength(10_000);
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
    expect(slowest).toBeLessThan(16);
  });
});
