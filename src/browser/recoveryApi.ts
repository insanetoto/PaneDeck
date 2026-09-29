import { invoke } from "@tauri-apps/api/core";

export interface RecoveryItem {
  recoveryId: string;
  jobId: number;
  kind: string;
  checkpoint: string;
  sourceItemsPresent: number;
  residualItems: number;
  cleanupAvailable: boolean;
}

export interface RecoveryClient {
  list(): Promise<RecoveryItem[]>;
  confirmCleanup(recoveryId: string): Promise<void>;
}

export const tauriRecoveryClient: RecoveryClient = {
  list: () => invoke("list_recovery_items"),
  confirmCleanup: (recoveryId) => invoke("confirm_recovery_cleanup", { request: { recoveryId } }),
};
