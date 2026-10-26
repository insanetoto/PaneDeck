import { invoke, isTauri } from "@tauri-apps/api/core";

export function openFullDiskAccessSettings(): Promise<void> {
  return isTauri() ? invoke("open_full_disk_access_settings") : Promise.resolve();
}
