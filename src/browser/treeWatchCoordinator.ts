import type { DirectoryReference } from "../ipc/pathReferences";
import type { PaneId } from "./directoryApi";
import type { DirectoryMonitor, DirectoryWatchReason } from "./directoryMonitor";
export const TREE_WATCH_BUDGET = 128;
const RESCAN_DELAY_MS = 150;
export interface WatchTarget {
  key: string;
  directory: DirectoryReference;
  rescan: () => Promise<void>;
  failed: (reason: "watchFailed" | "resourceLimit") => void;
}
interface Active {
  target: WatchTarget;
  stop?: () => void;
  timer?: ReturnType<typeof setTimeout>;
  scanning: boolean;
  dirty: boolean;
  failed: boolean;
}
/** Each signal is only a hint: merge bursts, then fully rescan that directory. */
export class TreeWatchCoordinator {
  private watches = new Map<string, Active>();
  private disposed = false;
  constructor(
    private monitor: DirectoryMonitor,
    private pane: PaneId,
    private budget = TREE_WATCH_BUDGET,
  ) {}
  private live(state: Active) {
    return !this.disposed && this.watches.get(state.target.key) === state;
  }
  private remove(key: string, state: Active) {
    this.watches.delete(key);
    if (state.timer) clearTimeout(state.timer);
    state.stop?.();
  }
  sync(targets: WatchTarget[]) {
    if (this.disposed) return;
    const wanted = new Set(targets.map((target) => target.key));
    for (const [key, state] of this.watches) if (!wanted.has(key)) this.remove(key, state);
    let allocated = [...this.watches.values()].filter((state) => !state.failed).length;
    for (const target of targets) {
      const existing = this.watches.get(target.key);
      if (existing) {
        existing.target = target;
        continue;
      }
      const state: Active = {
        target,
        scanning: false,
        dirty: false,
        failed: allocated >= this.budget,
      };
      this.watches.set(target.key, state);
      if (state.failed) {
        queueMicrotask(() => {
          if (this.live(state)) state.target.failed("resourceLimit");
        });
        continue;
      }
      allocated++;
      void this.monitor
        .start(this.pane, target.directory, (reason) => this.signal(state, reason))
        .then((stop) => {
          if (!this.live(state) || state.failed) stop();
          else state.stop = stop;
        })
        .catch(() => {
          if (!this.live(state)) return;
          state.failed = true;
          state.target.failed("watchFailed");
        });
    }
  }
  private signal(state: Active, reason: DirectoryWatchReason) {
    if (!this.live(state) || state.failed) return;
    if (reason === "watchFailed") {
      state.failed = true;
      state.stop?.();
      state.stop = undefined;
      if (state.timer) clearTimeout(state.timer);
      state.target.failed("watchFailed");
      // One last rescan; no automatic restart loop after native failure.
      if (!state.scanning) void this.scan(state);
      return;
    }
    state.dirty = true;
    if (state.scanning || state.timer) return;
    state.timer = setTimeout(() => {
      state.timer = undefined;
      void this.scan(state);
    }, RESCAN_DELAY_MS);
  }
  private async scan(state: Active) {
    if (!this.live(state) || state.scanning) return;
    state.scanning = true;
    state.dirty = false;
    try {
      await state.target.rescan();
    } catch {
      /* The owning branch renders its read error. */
    } finally {
      state.scanning = false;
      if (this.live(state) && state.dirty && !state.failed) this.signal(state, "changed");
    }
  }
  retry(key?: string) {
    for (const [id, state] of this.watches)
      if ((!key || id === key) && state.failed) this.remove(id, state);
  }
  dispose() {
    this.disposed = true;
    for (const [key, state] of this.watches) this.remove(key, state);
  }
}
