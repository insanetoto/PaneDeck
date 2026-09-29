import { invoke } from "@tauri-apps/api/core";

export interface DiagnosticPreview {
  fileName: string;
  content: string;
}

export interface DiagnosticClient {
  preview(): Promise<DiagnosticPreview>;
  export(destination: string, content: string): Promise<void>;
}

export const tauriDiagnosticClient: DiagnosticClient = {
  preview: () => invoke("preview_diagnostics"),
  export: (destination, content) =>
    invoke("export_diagnostics", { request: { destination, content } }),
};
