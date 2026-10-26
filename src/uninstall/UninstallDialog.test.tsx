import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { I18nProvider } from "../i18n/I18nProvider";
import type { UninstallClient } from "../browser/uninstallApi";
import { UninstallDialog } from "./UninstallDialog";

function client(overrides: Partial<UninstallClient> = {}): UninstallClient {
  return {
    preview: vi.fn(async () => ({
      paths: [
        {
          kind: "applicationSupport" as const,
          path: "/Users/test/Library/Application Support/dev.panedeck.desktop",
          exists: true,
          byteLen: "2048",
        },
        {
          kind: "caches" as const,
          path: "/Users/test/Library/Caches/dev.panedeck.desktop",
          exists: false,
          byteLen: "0",
        },
      ],
      appBundle: "/Applications/PaneDeck.app",
      appBundleExists: true,
      appBundleByteLen: "1024",
    })),
    uninstall: vi.fn(async () => ({
      appTrashed: true,
      appBundle: "/Applications/PaneDeck.app",
      removed: ["applicationSupport" as const],
      failed: [],
    })),
    quit: vi.fn(async () => undefined),
    ...overrides,
  };
}

describe("UninstallDialog", () => {
  afterEach(cleanup);

  it("shows the allowlisted paths and removes only after confirmation", async () => {
    const api = client();
    render(
      <I18nProvider>
        <UninstallDialog client={api} onClose={vi.fn()} open />
      </I18nProvider>,
    );

    expect(await screen.findByText("Application Support (logs)")).toBeInTheDocument();
    expect(screen.getByText("/Applications/PaneDeck.app")).toBeInTheDocument();
    expect(api.uninstall).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Uninstall and quit" }));
    await waitFor(() =>
      expect(api.uninstall).toHaveBeenCalledWith({ removeData: true, trashApp: true }),
    );
    expect(
      await screen.findByText("App data was processed. PaneDeck is ready to quit."),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Quit PaneDeck" }));
    expect(api.quit).toHaveBeenCalledOnce();
  });

  it("requires an explicit action when the bundle is unavailable", async () => {
    const api = client({
      preview: vi.fn(async () => ({
        paths: [],
        appBundle: null,
        appBundleExists: false,
        appBundleByteLen: null,
      })),
    });
    render(
      <I18nProvider>
        <UninstallDialog client={api} onClose={vi.fn()} open />
      </I18nProvider>,
    );
    expect(
      await screen.findByText("Not running from an app bundle; only app data can be removed."),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText("Delete PaneDeck app data"));
    expect(screen.getByRole("button", { name: "Uninstall and quit" })).toBeDisabled();
  });
});
