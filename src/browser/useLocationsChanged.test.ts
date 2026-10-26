import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useLocationsChanged } from "./useLocationsChanged";

const { listenMock } = vi.hoisted(() => ({
  listenMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ isTauri: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));

afterEach(() => {
  cleanup();
  listenMock.mockReset();
});

describe("useLocationsChanged", () => {
  it("subscribes to the native signal and removes the listener on unmount", async () => {
    let handler: (() => void) | undefined;
    const stop = vi.fn();
    listenMock.mockImplementation(async (_event: string, callback: () => void) => {
      handler = callback;
      return stop;
    });
    const onChange = vi.fn();
    const { unmount } = renderHook(() => useLocationsChanged(onChange));
    await waitFor(() => expect(handler).toBeDefined());

    act(() => handler?.());
    expect(onChange).toHaveBeenCalledOnce();
    expect(listenMock).toHaveBeenCalledWith("locations-changed", expect.any(Function));

    unmount();
    expect(stop).toHaveBeenCalledOnce();
  });
});
