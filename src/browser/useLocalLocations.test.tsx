import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LocalLocation, LocalLocations, LocalLocationsClient } from "./locationsApi";
import { useLocalLocations } from "./useLocalLocations";

function location(id: string, name: string): LocalLocation {
  return {
    id,
    kind: "localVolume",
    source: "volume",
    name,
    path: `/Volumes/${name}`,
    available: true,
    isLocal: true,
    isRemovable: false,
  };
}

function result(locations: LocalLocation[], warnings: LocalLocations["warnings"] = []) {
  return { locations, warnings };
}

afterEach(cleanup);

describe("useLocalLocations", () => {
  it("loads the shared snapshot once and exposes refresh", async () => {
    const client: LocalLocationsClient = {
      list: vi.fn().mockResolvedValue(result([location("volume:a", "Alpha")])),
    };
    const { result: hook } = renderHook(() => useLocalLocations(client));
    expect(hook.current.loading).toBe(true);
    await waitFor(() => expect(hook.current.loading).toBe(false));
    expect(hook.current.locations.map((item) => item.name)).toEqual(["Alpha"]);
    expect(client.list).toHaveBeenCalledWith(false);

    act(() => hook.current.refresh());
    await waitFor(() => expect(client.list).toHaveBeenCalledWith(true));
  });

  it("ignores a stale response that resolves after a newer request", async () => {
    const pending: ((value: LocalLocations) => void)[] = [];
    const client: LocalLocationsClient = {
      list: vi.fn(
        () =>
          new Promise<LocalLocations>((resolve) => {
            pending.push(resolve);
          }),
      ),
    };
    const { result: hook } = renderHook(() => useLocalLocations(client));
    await waitFor(() => expect(pending).toHaveLength(1));
    act(() => hook.current.refresh());
    expect(pending).toHaveLength(2);

    act(() => pending[1](result([location("volume:new", "New")])));
    await waitFor(() => expect(hook.current.locations[0]?.name).toBe("New"));
    act(() => pending[0](result([location("volume:old", "Old")])));
    await waitFor(() => expect(hook.current.locations[0]?.name).toBe("New"));
  });

  it("retains the last snapshot and reports a failed refresh", async () => {
    const client: LocalLocationsClient = {
      list: vi
        .fn()
        .mockResolvedValueOnce(result([location("volume:a", "Alpha")]))
        .mockRejectedValueOnce(new Error("offline")),
    };
    const { result: hook } = renderHook(() => useLocalLocations(client));
    await waitFor(() => expect(hook.current.locations).toHaveLength(1));

    act(() => hook.current.refresh());
    await waitFor(() => expect(hook.current.warnings).toContain("refreshFailed"));
    expect(hook.current.locations[0]?.name).toBe("Alpha");
    expect(hook.current.loading).toBe(false);
  });

  it("throttles focus refresh but keeps manual refresh immediate", async () => {
    const client: LocalLocationsClient = {
      list: vi.fn().mockResolvedValue(result([])),
    };
    const { result: hook } = renderHook(() => useLocalLocations(client));
    await waitFor(() => expect(client.list).toHaveBeenCalledTimes(1));

    act(() => window.dispatchEvent(new Event("focus")));
    expect(client.list).toHaveBeenCalledTimes(1);

    act(() => hook.current.refresh());
    expect(client.list).toHaveBeenCalledTimes(2);
    expect(client.list).toHaveBeenLastCalledWith(true);
  });
});
