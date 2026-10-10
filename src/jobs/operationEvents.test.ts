import { describe, expect, it, vi } from "vitest";
import {
  watchOperations,
  type OperationSnapshot,
  type OperationWatchTransport,
} from "../browser/operationApi";
const snapshot = (sequence: number): OperationSnapshot => ({
  sequence,
  jobs: [],
  logStatus: { persistenceEnabled: true, failedWrites: 0, lastError: null },
  recoveryLogStatus: { persistenceEnabled: true, failedWrites: 0, lastError: null },
});
describe("operation watch", () => {
  it("listens before snapshot, buffers racing updates and rejects old or unrelated events", async () => {
    let receive!: (token: string, snapshot: OperationSnapshot) => void,
      token = "";
    let initial!: (value: [number, OperationSnapshot]) => void;
    const transport: OperationWatchTransport = {
      listen: vi.fn(async (callback) => {
        receive = callback;
        return stopListen;
      }),
      start: vi.fn((scope) => {
        token = scope;
        return new Promise<[number, OperationSnapshot]>((resolve) => {
          initial = resolve;
        });
      }),
      stop: vi.fn(async () => {}),
    };
    const stopListen = vi.fn(),
      apply = vi.fn();
    const pending = watchOperations(transport, apply);
    await Promise.resolve();
    receive("foreign", snapshot(10));
    receive(token, snapshot(3));
    initial([1, snapshot(1)]);
    const cleanup = await pending;
    expect(apply).toHaveBeenCalledExactlyOnceWith(snapshot(3));
    receive(token, snapshot(2));
    receive(token, snapshot(4));
    expect(apply).toHaveBeenCalledTimes(2);
    cleanup();
    cleanup();
    receive(token, snapshot(5));
    expect(stopListen).toHaveBeenCalledTimes(1);
    expect(transport.stop).toHaveBeenCalledExactlyOnceWith(1);
  });
  it("releases the event listener when start fails", async () => {
    const stop = vi.fn();
    const transport: OperationWatchTransport = {
      listen: async () => stop,
      start: async () => {
        throw Error("offline");
      },
      stop: vi.fn(),
    };
    await expect(watchOperations(transport, vi.fn())).rejects.toThrow("offline");
    expect(stop).toHaveBeenCalledTimes(1);
  });
});
