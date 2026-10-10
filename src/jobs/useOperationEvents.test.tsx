import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { OperationClient, OperationSnapshot } from "../browser/operationApi";
import { useOperationEvents } from "./useOperationEvents";
afterEach(cleanup);
it("reconnects on focus, rejects stale callbacks and cleans up late subscriptions", async () => {
  const callbacks: Array<(snapshot: OperationSnapshot) => void> = [];
  const starts: Array<(stop: () => void) => void> = [];
  const client = {
    watch: vi.fn((receive) => {
      callbacks.push(receive);
      return new Promise<() => void>((resolve) => starts.push(resolve));
    }),
  } as unknown as OperationClient;
  const receive = vi.fn(),
    failed = vi.fn();
  const view = renderHook(() => useOperationEvents(client, receive, failed));
  expect(client.watch).toHaveBeenCalledTimes(1);
  act(() => window.dispatchEvent(new Event("focus")));
  const obsolete = vi.fn(),
    current = vi.fn();
  await act(async () => starts[0](obsolete));
  expect(obsolete).toHaveBeenCalledOnce();
  const snapshot = { sequence: 1, jobs: [] } as unknown as OperationSnapshot;
  act(() => {
    callbacks[0](snapshot);
    callbacks[1](snapshot);
  });
  expect(receive).toHaveBeenCalledExactlyOnceWith(snapshot);
  await act(async () => starts[1](current));
  act(() => window.dispatchEvent(new Event("focus")));
  expect(current).toHaveBeenCalledOnce();
  view.unmount();
  const late = vi.fn();
  await act(async () => starts[2](late));
  expect(late).toHaveBeenCalledOnce();
  window.dispatchEvent(new Event("focus"));
  expect(client.watch).toHaveBeenCalledTimes(3);
  expect(failed).not.toHaveBeenCalled();
});
