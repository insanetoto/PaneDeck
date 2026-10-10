import { beforeEach, describe, expect, it, vi } from "vitest";
const bridge = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: bridge.listen }));
import { tauriDirectoryMonitor } from "./directoryMonitor";
interface Payload {
  pane: string;
  watchId: number;
  reason: string;
}
beforeEach(() => vi.resetAllMocks());
describe("directory monitor IPC lifecycle", () => {
  it("buffers startup events, filters IDs/panes and stops exactly once", async () => {
    let emit!: (event: { payload: Payload }) => void;
    const unlisten = vi.fn(),
      rescan = vi.fn();
    bridge.listen.mockImplementation(async (_name, callback) => {
      emit = callback;
      return unlisten;
    });
    bridge.invoke.mockImplementation(async (command) => {
      if (command === "start_directory_watch") {
        emit({ payload: { pane: "left", watchId: 22, reason: "changed" } });
        emit({ payload: { pane: "left", watchId: 21, reason: "changed" } });
        emit({ payload: { pane: "right", watchId: 22, reason: "changed" } });
        return 22;
      }
    });
    const stop = await tauriDirectoryMonitor.start("left", { sessionId: "1" }, rescan);
    expect(rescan).toHaveBeenCalledTimes(1);
    emit({ payload: { pane: "left", watchId: 21, reason: "changed" } });
    emit({ payload: { pane: "left", watchId: 22, reason: "watchFailed" } });
    expect(rescan).toHaveBeenCalledTimes(2);
    stop();
    stop();
    emit({ payload: { pane: "left", watchId: 22, reason: "changed" } });
    expect(rescan).toHaveBeenCalledTimes(2);
    expect(unlisten).toHaveBeenCalledOnce();
    expect(
      bridge.invoke.mock.calls.filter(([name]) => name === "stop_directory_watch"),
    ).toHaveLength(1);
  });
  it("releases its subscription when watch creation fails", async () => {
    const unlisten = vi.fn();
    bridge.listen.mockResolvedValue(unlisten);
    bridge.invoke.mockRejectedValue({ code: "resourceLimit" });
    await expect(tauriDirectoryMonitor.start("left", { sessionId: "1" }, vi.fn())).rejects.toEqual({
      code: "resourceLimit",
    });
    expect(unlisten).toHaveBeenCalledOnce();
  });
});
