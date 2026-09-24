import { invoke } from "@tauri-apps/api/core";

export type LocalLocationKind = "home" | "desktop" | "downloads" | "documents" | "volume";

export interface LocalLocation {
  id: string;
  kind: LocalLocationKind;
  name: string;
  path: string;
  available: boolean;
}

export interface LocalLocations {
  locations: LocalLocation[];
  volumeScanFailed: boolean;
}

export interface LocalLocationsClient {
  list(): Promise<LocalLocations>;
}

export const tauriLocalLocationsClient: LocalLocationsClient = {
  list: () => invoke("list_local_locations"),
};
