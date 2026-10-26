import { describe, expect, it } from "vitest";
import {
  COLUMN_WIDTHS_STORAGE_KEY,
  DEFAULT_COLUMN_WIDTHS,
  clampColumnWidth,
  loadColumnWidths,
  sanitizeColumnWidths,
  saveColumnWidths,
  tableMinWidth,
} from "./columnWidths";

function memoryStorage(): Storage {
  const values = new Map<string, string>();
  return {
    get length() {
      return values.size;
    },
    clear: () => values.clear(),
    getItem: (key) => values.get(key) ?? null,
    key: (index) => [...values.keys()][index] ?? null,
    removeItem: (key) => {
      values.delete(key);
    },
    setItem: (key, value) => {
      values.set(key, String(value));
    },
  };
}

describe("column width model", () => {
  it("uses an automatic name column and the reviewed metadata defaults", () => {
    expect(DEFAULT_COLUMN_WIDTHS).toEqual({ name: null, size: 92, kind: 92, modifiedAt: 150 });
    expect(tableMinWidth(DEFAULT_COLUMN_WIDTHS)).toBe(520);
    expect(tableMinWidth({ name: 300, size: 92, kind: 92, modifiedAt: 150 })).toBe(634);
  });

  it("clamps every column to its documented limits and never returns NaN", () => {
    expect(clampColumnWidth("name", 10)).toBe(180);
    expect(clampColumnWidth("name", 900)).toBe(720);
    expect(clampColumnWidth("size", 0)).toBe(64);
    expect(clampColumnWidth("kind", 999)).toBe(320);
    expect(clampColumnWidth("modifiedAt", Number.NaN)).toBe(120);
    expect(clampColumnWidth("modifiedAt", 150.4)).toBe(150);
  });

  it("sanitizes corrupt fields independently and falls back to automatic name", () => {
    expect(
      sanitizeColumnWidths({
        name: "300",
        size: 12,
        kind: 200,
        modifiedAt: null,
      }),
    ).toEqual({ name: null, size: 64, kind: 200, modifiedAt: 150 });
    expect(sanitizeColumnWidths(null)).toEqual(DEFAULT_COLUMN_WIDTHS);
    expect(sanitizeColumnWidths({ name: 400 })).toEqual({
      ...DEFAULT_COLUMN_WIDTHS,
      name: 400,
    });
  });

  it("persists each pane independently and preserves the other pane", () => {
    const storage = memoryStorage();
    saveColumnWidths("left", { name: 320, size: 120, kind: 92, modifiedAt: 150 }, storage);
    saveColumnWidths("right", { name: null, size: 92, kind: 140, modifiedAt: 220 }, storage);

    expect(loadColumnWidths("left", storage)).toEqual({
      name: 320,
      size: 120,
      kind: 92,
      modifiedAt: 150,
    });
    expect(loadColumnWidths("right", storage)).toEqual({
      name: null,
      size: 92,
      kind: 140,
      modifiedAt: 220,
    });
    expect(loadColumnWidths("left", memoryStorage())).toEqual(DEFAULT_COLUMN_WIDTHS);
  });

  it("ignores corrupt JSON, older schemas and storage failures", () => {
    const corrupt = memoryStorage();
    corrupt.setItem(COLUMN_WIDTHS_STORAGE_KEY, "{not-json");
    expect(loadColumnWidths("left", corrupt)).toEqual(DEFAULT_COLUMN_WIDTHS);

    const older = memoryStorage();
    older.setItem(
      COLUMN_WIDTHS_STORAGE_KEY,
      JSON.stringify({ version: 0, panes: { left: { size: 300 } } }),
    );
    expect(loadColumnWidths("left", older)).toEqual(DEFAULT_COLUMN_WIDTHS);

    const missing = memoryStorage();
    expect(loadColumnWidths("left", missing)).toEqual(DEFAULT_COLUMN_WIDTHS);

    const failingRead: Storage = {
      ...memoryStorage(),
      getItem: () => {
        throw new Error("storage disabled");
      },
    };
    expect(loadColumnWidths("right", failingRead)).toEqual(DEFAULT_COLUMN_WIDTHS);

    const failingWrite: Storage = {
      ...memoryStorage(),
      setItem: () => {
        throw new Error("quota exceeded");
      },
    };
    expect(() => saveColumnWidths("left", DEFAULT_COLUMN_WIDTHS, failingWrite)).not.toThrow();
  });

  it("reads out-of-range saved values back at the nearest safe limit", () => {
    const storage = memoryStorage();
    storage.setItem(
      COLUMN_WIDTHS_STORAGE_KEY,
      JSON.stringify({
        version: 1,
        panes: {
          left: { name: 9999, size: 1, kind: 9999, modifiedAt: 1 },
        },
      }),
    );
    expect(loadColumnWidths("left", storage)).toEqual({
      name: 720,
      size: 64,
      kind: 320,
      modifiedAt: 120,
    });
  });
});
