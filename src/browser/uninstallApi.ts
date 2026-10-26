import { invoke, isTauri } from "@tauri-apps/api/core";

export type UninstallDataKind =
  | "applicationSupport"
  | "caches"
  | "preferences"
  | "webKit"
  | "httpStorages"
  | "savedState"
  | "logs";

export interface UninstallPath {
  kind: UninstallDataKind;
  path: string;
  exists: boolean;
  byteLen: string;
}

export interface UninstallPreview {
  paths: UninstallPath[];
  appBundle: string | null;
  appBundleExists: boolean;
  appBundleByteLen: string | null;
}

export interface UninstallRequest {
  removeData: boolean;
  trashApp: boolean;
}

export interface UninstallResult {
  appTrashed: boolean;
  appBundle: string | null;
  removed: UninstallDataKind[];
  failed: string[];
}

export interface UninstallClient {
  preview(): Promise<UninstallPreview>;
  uninstall(request: UninstallRequest): Promise<UninstallResult>;
  quit(): Promise<void>;
}

export const tauriUninstallClient: UninstallClient = {
  preview: () => invoke("preview_uninstall"),
  uninstall: (request) => invoke("uninstall_app", { request }),
  quit: () => invoke("quit_app"),
};

export const unavailableUninstallClient: UninstallClient = {
  preview: () =>
    Promise.resolve({
      paths: [],
      appBundle: null,
      appBundleExists: false,
      appBundleByteLen: null,
    }),
  uninstall: () => Promise.reject(new Error("uninstall is unavailable")),
  quit: () => Promise.resolve(),
};

export function uninstallAvailable(): boolean {
  return isTauri();
}
