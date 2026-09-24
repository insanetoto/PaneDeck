import { beforeEach, describe, expect, it } from "vitest";
import { FAVORITES_STORAGE_KEY, favoriteForPath, loadFavorites, saveFavorites } from "./favorites";

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
});
