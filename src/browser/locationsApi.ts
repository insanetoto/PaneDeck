import { invoke, isTauri } from "@tauri-apps/api/core";

export type LocalLocationKind =
  | "home"
  | "desktop"
  | "downloads"
  | "documents"
  | "iCloudDrive"
  | "cloudStorage"
  | "localVolume"
  | "externalVolume"
  | "networkVolume";

export type LocalLocationSource = "standard" | "iCloud" | "fileProvider" | "volume";

export type LocalLocationWarning =
  "volumes" | "iCloud" | "cloudStorage" | "timeout" | "refreshFailed";

export interface LocalLocation {
  id: string;
  kind: LocalLocationKind;
  source: LocalLocationSource;
  name: string;
  path: string;
  available: boolean;
  isLocal: boolean;
  isRemovable: boolean;
}

export interface LocalLocations {
  locations: LocalLocation[];
  warnings: LocalLocationWarning[];
}

export interface LocalLocationsClient {
  /** `refresh` forces a new discovery; the backend may still return a cache. */
  list(refresh?: boolean): Promise<LocalLocations>;
}

export const tauriLocalLocationsClient: LocalLocationsClient = {
  list: async (refresh = false) => {
    if (!isTauri()) return { locations: [], warnings: [] };
    return invoke("list_local_locations", { refresh });
  },
};
