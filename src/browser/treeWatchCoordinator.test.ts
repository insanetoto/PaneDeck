import { afterEach, describe, expect, it, vi } from "vitest";
import type { DirectoryMonitor, DirectoryWatchReason } from "./directoryMonitor";
import { TreeWatchCoordinator, type WatchTarget } from "./treeWatchCoordinator";
function setup(budget = 128) {
  const signals = new Map<string, (reason: DirectoryWatchReason) => void>();
  const stops = new Map<string, ReturnType<typeof vi.fn>>();
  const monitor: DirectoryMonitor = {
    start: vi.fn(async (_pane, directory, callback) => {
      signals.set(directory.sessionId, callback);
      const stop = vi.fn();
      stops.set(directory.sessionId, stop);
      return stop;
    }),
  };
  const coordinator = new TreeWatchCoordinator(monitor, "left", budget);
  const target = (key: string): WatchTarget => ({
    key,
    directory: { sessionId: key },
    rescan: vi.fn(async () => {}),
    failed: vi.fn(),
  });
  return { signals, stops, monitor, coordinator, target };
}
afterEach(() => vi.useRealTimers());
describe("tree watch coordinator", () => {
  it("merges storms and follows a signal during an in-flight scan with one full rescan", async () => {
    vi.useFakeTimers();
    const { coordinator, target, signals } = setup();
    const root = target("root");
    let resolve!: () => void;
    root.rescan = vi.fn(
      () =>
        new Promise<void>((finish) => {
          resolve = finish;
        }),
    );
    coordinator.sync([root]);
    await Promise.resolve();
    for (let i = 0; i < 100; i++) signals.get("root")!("changed");
    await vi.advanceTimersByTimeAsync(150);
    expect(root.rescan).toHaveBeenCalledTimes(1);
    for (let i = 0; i < 100; i++) signals.get("root")!("changed");
    resolve();
    await vi.advanceTimersByTimeAsync(150);
    expect(root.rescan).toHaveBeenCalledTimes(2);
    resolve();
    await vi.advanceTimersByTimeAsync(1000);
    expect(root.rescan).toHaveBeenCalledTimes(2);
    coordinator.dispose();
  });
  it("stops collapsed branches, ignores old signals and watches again when expanded", async () => {
    vi.useFakeTimers();
    const { coordinator, target, signals, stops, monitor } = setup();
    const root = target("root"),
      branch = target("branch");
    coordinator.sync([root, branch]);
    await Promise.resolve();
    const old = signals.get("branch")!;
    coordinator.sync([root]);
    expect(stops.get("branch")).toHaveBeenCalledTimes(1);
    old("changed");
    await vi.advanceTimersByTimeAsync(500);
    expect(branch.rescan).not.toHaveBeenCalled();
    coordinator.sync([root, branch]);
    await Promise.resolve();
    expect(monitor.start).toHaveBeenCalledTimes(3);
    coordinator.dispose();
    expect(stops.get("root")).toHaveBeenCalledTimes(1);
  });
  it("releases a watch whose asynchronous startup completes after navigation", async () => {
    let finish!: (stop: () => void) => void;
    const monitor: DirectoryMonitor = {
      start: vi.fn(
        () =>
          new Promise<() => void>((resolve) => {
            finish = resolve;
          }),
      ),
    };
    const coordinator = new TreeWatchCoordinator(monitor, "left");
    const stop = vi.fn();
    coordinator.sync([
      { key: "old", directory: { sessionId: "1" }, rescan: vi.fn(), failed: vi.fn() },
    ]);
    coordinator.dispose();
    finish(stop);
    await Promise.resolve();
    expect(stop).toHaveBeenCalledOnce();
  });
  it("exposes budget and native failures without automatically restarting them", async () => {
    vi.useFakeTimers();
    const { coordinator, target, signals, monitor } = setup(1);
    const root = target("root"),
      branch = target("branch");
    coordinator.sync([root, branch]);
    await vi.advanceTimersByTimeAsync(0);
    expect(branch.failed).toHaveBeenCalledWith("resourceLimit");
    expect(monitor.start).toHaveBeenCalledOnce();
    signals.get("root")!("watchFailed");
    await vi.advanceTimersByTimeAsync(1000);
    expect(root.failed).toHaveBeenCalledWith("watchFailed");
    expect(root.rescan).toHaveBeenCalledOnce();
    coordinator.sync([root, branch]);
    expect(monitor.start).toHaveBeenCalledOnce();
    coordinator.retry("root");
    coordinator.sync([root, branch]);
    await Promise.resolve();
    expect(monitor.start).toHaveBeenCalledTimes(2);
    coordinator.dispose();
  });
});
