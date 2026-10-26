import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { FinderFavorite, FinderFavorites, FinderFavoritesClient } from "./finderFavoritesApi";
import { useFinderFavorites } from "./useFinderFavorites";

function favorite(id: string, name: string): FinderFavorite {
  return { id, name, path: `/Users/test/${name}`, available: true };
}

function result(
  favorites: FinderFavorite[],
  extra: Partial<FinderFavorites> = {},
): FinderFavorites {
  return { favorites, unresolved: 0, failed: false, timedOut: false, ...extra };
}

afterEach(cleanup);

describe("useFinderFavorites", () => {
  it("loads once, exposes refresh and keeps unresolved counts", async () => {
    const client: FinderFavoritesClient = {
      list: vi.fn().mockResolvedValue(result([favorite("finder:a", "Alpha")], { unresolved: 2 })),
    };
    const { result: hook } = renderHook(() => useFinderFavorites(client));
    await waitFor(() => expect(hook.current.loading).toBe(false));
    expect(hook.current.favorites.map((item) => item.name)).toEqual(["Alpha"]);
    expect(hook.current.unresolved).toBe(2);
    expect(client.list).toHaveBeenCalledWith(false);

    act(() => hook.current.refresh());
    await waitFor(() => expect(client.list).toHaveBeenCalledWith(true));
  });

  it("ignores a stale response and keeps the last list after failure", async () => {
    const pending: ((value: FinderFavorites) => void)[] = [];
    const client: FinderFavoritesClient = {
      list: vi.fn(
        () =>
          new Promise<FinderFavorites>((resolve) => {
            pending.push(resolve);
          }),
      ),
    };
    const { result: hook } = renderHook(() => useFinderFavorites(client));
    await waitFor(() => expect(pending).toHaveLength(1));
    act(() => hook.current.refresh());
    act(() => pending[1](result([favorite("finder:new", "New")])));
    await waitFor(() => expect(hook.current.favorites[0]?.name).toBe("New"));
    act(() => pending[0](result([favorite("finder:old", "Old")])));
    await waitFor(() => expect(hook.current.favorites[0]?.name).toBe("New"));
  });

  it("retains the last snapshot and marks a failed refresh", async () => {
    const client: FinderFavoritesClient = {
      list: vi
        .fn()
        .mockResolvedValueOnce(result([favorite("finder:a", "Alpha")]))
        .mockRejectedValueOnce(new Error("unavailable")),
    };
    const { result: hook } = renderHook(() => useFinderFavorites(client));
    await waitFor(() => expect(hook.current.favorites).toHaveLength(1));
    act(() => hook.current.refresh());
    await waitFor(() => expect(hook.current.failed).toBe(true));
    expect(hook.current.favorites[0]?.name).toBe("Alpha");
  });

  it("throttles focus refresh but not manual refresh", async () => {
    const client: FinderFavoritesClient = { list: vi.fn().mockResolvedValue(result([])) };
    const { result: hook } = renderHook(() => useFinderFavorites(client));
    await waitFor(() => expect(client.list).toHaveBeenCalledTimes(1));
    act(() => window.dispatchEvent(new Event("focus")));
    expect(client.list).toHaveBeenCalledTimes(1);
    act(() => hook.current.refresh());
    expect(client.list).toHaveBeenCalledTimes(2);
  });
});
