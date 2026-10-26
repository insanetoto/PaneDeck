import { beforeEach, describe, expect, it } from "vitest";
import {
  FAVORITES_STORAGE_KEY,
  favoriteForPath,
  loadFavorites,
  mergeFavorites,
  saveFavorites,
} from "./favorites";

describe("local favorites", () => {
  beforeEach(() => window.localStorage.clear());

  it("persists only local absolute paths without network activity", () => {
    saveFavorites([favoriteForPath("/Users/test/Work")]);
    expect(loadFavorites()).toEqual([{ name: "Work", path: "/Users/test/Work" }]);
    expect(window.localStorage.getItem(FAVORITES_STORAGE_KEY)).toContain("/Users/test/Work");
  });

  it("ignores malformed, relative, and duplicate records", () => {
    window.localStorage.setItem(
      FAVORITES_STORAGE_KEY,
      JSON.stringify([
        { name: "bad", path: "relative" },
        { name: "First", path: "/safe" },
        { name: "Last", path: "/safe" },
        { cloudId: "forbidden" },
      ]),
    );
    expect(loadFavorites()).toEqual([{ name: "Last", path: "/safe" }]);
  });

  it("merges Finder order and names with app-only favorites", () => {
    const merged = mergeFavorites(
      [
        { id: "finder:/one", name: "One", path: "/one", available: true },
        { id: "finder:/two", name: "Two", path: "/two", available: true },
      ],
      [
        { name: "App two", path: "/two" },
        { name: "App three", path: "/three" },
      ],
    );
    expect(merged).toEqual([
      { name: "One", path: "/one", source: "finder", available: true },
      { name: "Two", path: "/two", source: "app", available: true },
      { name: "App three", path: "/three", source: "app", available: true },
    ]);
  });

  it("keeps the same name for different paths and never duplicates a path", () => {
    const merged = mergeFavorites(
      [
        { id: "finder:/a/work", name: "Work", path: "/a/work", available: true },
        { id: "finder:/a/work", name: "Duplicate", path: "/a/work", available: true },
      ],
      [
        { name: "Work", path: "/b/work" },
        { name: "Work", path: "/a/work" },
      ],
    );
    expect(merged.map((favorite) => favorite.path)).toEqual(["/a/work", "/b/work"]);
    expect(merged.map((favorite) => favorite.name)).toEqual(["Work", "Work"]);
  });
});
