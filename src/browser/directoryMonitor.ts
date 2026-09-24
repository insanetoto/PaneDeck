import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DirectoryReference } from "../ipc/pathReferences";
import type { PaneId } from "./directoryApi";

export type DirectoryWatchReason = "changed" | "watchFailed";

interface DirectoryWatchEvent {
  pane: PaneId;
  watchId: number;
  reason: DirectoryWatchReason;
}

export interface DirectoryMonitor {
  start(
    pane: PaneId,
    directory: DirectoryReference,
    onRescan: (reason: DirectoryWatchReason) => void,
  ): Promise<() => void>;
}

export const tauriDirectoryMonitor: DirectoryMonitor = {
  async start(pane, directory, onRescan) {
    let watchId: number | null = null;
    const unlisten = await listen<DirectoryWatchEvent>("directory-rescan", ({ payload }) => {
      if (payload.pane === pane && payload.watchId === watchId) onRescan(payload.reason);
    });
    try {
      watchId = await invoke<number>("start_directory_watch", {
        request: { pane, directory },
      });
    } catch (error) {
      unlisten();
      throw error;
    }
    return () => {
      unlisten();
      void invoke("stop_directory_watch", { request: { pane, watchId } });
    };
  },
};
