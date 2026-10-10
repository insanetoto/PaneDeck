import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import App from "../App";
import { I18nProvider, LANGUAGE_STORAGE_KEY } from "../i18n/I18nProvider";
import type { DirectoryClient, DirectoryListing } from "../browser/directoryApi";
import type { OperationClient, OperationJob } from "../browser/operationApi";
import type { ClipboardClient } from "../browser/clipboardApi";
import type { DesktopMenuClient, DesktopMenuEvent, DesktopMenuSnapshot } from "./desktopMenu";
import { captureCommandContext } from "./context";
import { dispatchWrite } from "./dispatch";

const listing = (pane: string): DirectoryListing => ({
  directory: { sessionId: pane },
  displayPath: `/${pane}`,
  canGoUp: true,
  warnings: [],
  entries: ["first", "second"].map((name, index) => ({
    reference: { sessionId: pane, entryId: String(index + 1) },
    displayName: `${pane}-${name}.txt`,
    displayNameIsLossy: false,
    kind: "file",
    byteLen: "1",
    modifiedAtUnixMillis: null,
    isHidden: false,
    isReadOnly: false,
    symlinkTarget: null,
  })),
});
const result: OperationJob = {
  jobId: 1,
  kind: "copy",
  state: "queued",
  itemCount: 1,
  completedItems: 0,
  failedItems: 0,
  completedUnits: null,
  totalUnits: null,
  bytesPerSecond: null,
  elapsedMillis: 0,
  itemOutcomes: [],
  canCancel: true,
};

async function setup(desktopMenuClient?: DesktopMenuClient) {
  const operationClient: OperationClient = {
    transfer: vi.fn().mockResolvedValue(result),
    transferToEntry: vi.fn().mockResolvedValue(result),
    rename: vi.fn().mockResolvedValue(result),
    createDirectory: vi.fn().mockResolvedValue(result),
    trash: vi.fn().mockResolvedValue(result),
    listJobs: vi.fn().mockResolvedValue([]),
    cancel: vi.fn(),
    resolveConflict: vi.fn(),
  };
  const clipboardClient: ClipboardClient = {
    set: vi
      .fn()
      .mockResolvedValue({ intent: "copy", sourcePane: "left", itemCount: 1, cutEntries: [] }),
    prepare: vi.fn().mockResolvedValue({
      draftId: 1,
      itemCount: 1,
      intent: "copy",
      targetPane: "right",
      destinationDisplayPath: "/right",
    }),
    confirm: vi.fn(),
    cancel: vi.fn(),
  };
  const diagnosticClient = {
    preview: vi.fn().mockResolvedValue({ fileName: "diagnostics.json", content: "{}" }),
    export: vi.fn().mockResolvedValue(undefined),
  };
  const directoryClient: DirectoryClient = {
    openPath: vi.fn((_path, options) => Promise.resolve(listing(options.pane))),
    reopen: vi.fn((_directory, options) => Promise.resolve(listing(options.pane))),
    openChild: vi.fn(),
    openParent: vi.fn(),
  };
  render(
    <I18nProvider>
      <App
        desktopMenuClient={desktopMenuClient}
        diagnosticClient={diagnosticClient}
        directoryClient={directoryClient}
        operationClient={operationClient}
        clipboardClient={clipboardClient}
        locationsClient={{ list: async () => ({ locations: [], volumeScanFailed: false }) }}
      />
    </I18nProvider>,
  );
  const left = screen.getByRole("region", { name: "Left pane" });
  const right = screen.getByRole("region", { name: "Right pane" });
  for (const pane of [left, right]) {
    const input = within(pane).getByRole("textbox", { name: "Folder path" });
    fireEvent.change(input, { target: { value: "/test" } });
    fireEvent.submit(input.closest("form")!);
  }
  await within(left).findByText("left-first.txt");
  await within(right).findByText("right-first.txt");
  return { left, right, operationClient, clipboardClient, directoryClient, diagnosticClient };
}

beforeEach(() => window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "en"));
afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

describe("captured command routing", () => {
  it("keeps right-click multi-selection and original target after focus changes", async () => {
    const { left, right, operationClient } = await setup();
    fireEvent.click(within(left).getByText("left-first.txt"));
    fireEvent.click(within(left).getByText("left-second.txt"), { metaKey: true });
    fireEvent.contextMenu(within(left).getByText("left-first.txt"));
    fireEvent.focus(right);
    fireEvent.click(
      within(screen.getByRole("menu")).getByRole("menuitem", { name: /Copy to Right Pane/ }),
    );
    await waitFor(() =>
      expect(operationClient.transfer).toHaveBeenCalledWith(
        "copy",
        [
          { sessionId: "left", entryId: "1" },
          { sessionId: "left", entryId: "2" },
        ],
        { sessionId: "right" },
      ),
    );
  });
  it("uses the blank-menu root for new-folder confirmation even after focus changes", async () => {
    const { left, right, operationClient } = await setup();
    fireEvent.contextMenu(left.querySelector(".file-list-spacer")!);
    fireEvent.click(within(screen.getByRole("menu")).getByRole("menuitem", { name: /New Folder/ }));
    fireEvent.focus(right);
    const name = within(screen.getByRole("dialog")).getByRole("textbox", { name: "Name" });
    fireEvent.change(name, { target: { value: "created" } });
    fireEvent.submit(name.closest("form")!);
    await waitFor(() =>
      expect(operationClient.createDirectory).toHaveBeenCalledWith(
        { sessionId: "left" },
        "created",
      ),
    );
  });
  it("pastes into the explicitly clicked blank area without following the clipboard default", async () => {
    const { left, right, clipboardClient } = await setup();
    fireEvent.click(within(left).getByText("left-first.txt"));
    fireEvent.keyDown(screen.getByRole("main"), { key: "c", metaKey: true });
    await waitFor(() => expect(clipboardClient.set).toHaveBeenCalled());
    fireEvent.contextMenu(left.querySelector(".file-list-spacer")!);
    fireEvent.focus(right);
    fireEvent.click(within(screen.getByRole("menu")).getByRole("menuitem", { name: /Paste/ }));
    await waitFor(() =>
      expect(clipboardClient.prepare).toHaveBeenCalledWith("left", { sessionId: "left" }),
    );
  });
  it("does not run file shortcuts in path or name inputs", async () => {
    const { left, operationClient, clipboardClient } = await setup();
    fireEvent.click(within(left).getByText("left-first.txt"));
    const input = within(left).getByRole("textbox", { name: "Folder path" });
    for (const event of [
      { key: "F5" },
      { key: "Backspace", metaKey: true },
      { key: "x", metaKey: true },
    ])
      fireEvent.keyDown(input, event);
    expect(operationClient.transfer).not.toHaveBeenCalled();
    expect(operationClient.trash).not.toHaveBeenCalled();
    expect(clipboardClient.set).not.toHaveBeenCalled();
  });
  it("rejects an invalid write before invoking the backend", async () => {
    const { operationClient } = await setup();
    const context = captureCommandContext("keyboard", "left", { left: null, right: null }, []);
    await expect(dispatchWrite(operationClient, "trash", context)).rejects.toThrow(
      "invalid command context",
    );
    expect(operationClient.trash).not.toHaveBeenCalled();
  });
});

function nativeMenu() {
  const snapshots: DesktopMenuSnapshot[] = [];
  let handler: ((event: DesktopMenuEvent) => void) | undefined;
  const unsubscribe = vi.fn();
  const client: DesktopMenuClient = {
    available: () => true,
    sync: vi.fn(async (snapshot) => {
      snapshots.push(snapshot);
    }),
    subscribe: vi.fn(async (callback) => {
      handler = callback;
      return unsubscribe;
    }),
  };
  let sequence = 0;
  return {
    client,
    snapshots,
    latest: () => snapshots[snapshots.length - 1],
    unsubscribe,
    send: (
      command: DesktopMenuEvent["command"],
      revision = snapshots[snapshots.length - 1]!.revision,
      eventId = String(sequence++),
    ) => {
      act(() => handler?.({ command, revision, eventId }));
      return eventId;
    },
  };
}

describe("native menu routing", () => {
  it("runs each accelerator only once and rejects old-menu events after focus changes", async () => {
    const menu = nativeMenu();
    const { left, right, operationClient, clipboardClient } = await setup(menu.client);
    await waitFor(() => expect(menu.latest()?.enabledCommands).toContain("newFolder"));
    expect(menu.latest()?.enabledCommands).not.toContain("trash");
    fireEvent.click(within(left).getByText("left-first.txt"));
    await waitFor(() => expect(menu.latest()?.enabledCommands).toContain("copyToOther"));
    const revision = menu.latest()!.revision;
    fireEvent.keyDown(screen.getByRole("main"), { key: "F5" });
    expect(operationClient.transfer).not.toHaveBeenCalled();
    const id = menu.send("copyToOther", revision);
    menu.send("copyToOther", revision, id);
    await waitFor(() => expect(operationClient.transfer).toHaveBeenCalledTimes(1));
    fireEvent.click(within(right).getByText("right-first.txt"));
    await waitFor(() => expect(menu.latest()!.revision).not.toBe(revision));
    menu.send("trash", revision);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await waitFor(() => expect(menu.latest()?.enabledCommands).toContain("copy"));
    menu.send("copy");
    await waitFor(() =>
      expect(clipboardClient.set).toHaveBeenCalledWith("right", "copy", [
        { sessionId: "right", entryId: "1" },
      ]),
    );
    cleanup();
    expect(menu.unsubscribe).toHaveBeenCalledTimes(1);
  });

  it("updates language immediately and routes view actions to the focused pane", async () => {
    const menu = nativeMenu();
    const { left, right, directoryClient } = await setup(menu.client);
    fireEvent.focus(left);
    await waitFor(() => expect(menu.latest()?.enabledCommands).toContain("toggleHidden"));
    menu.send("toggleHidden");
    await waitFor(() =>
      expect(within(left).getByRole("button", { name: "Show hidden items" })).toHaveAttribute(
        "aria-pressed",
        "true",
      ),
    );
    expect(within(right).getByRole("button", { name: "Show hidden items" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    await waitFor(() => expect(menu.latest()?.showHidden).toBe(true));
    menu.send("refresh");
    await waitFor(() =>
      expect(directoryClient.reopen).toHaveBeenCalledWith(
        { sessionId: "left" },
        expect.objectContaining({ pane: "left", showHidden: true }),
      ),
    );
    menu.send("languageChinese");
    await waitFor(() => expect(document.documentElement.lang).toBe("zh-CN"));
    await waitFor(() => expect(menu.latest()?.language).toBe("zh-CN"));
  });

  it("disables file commands while editing a path and reports sync failures", async () => {
    const menu = nativeMenu();
    const { left, operationClient, clipboardClient } = await setup(menu.client);
    fireEvent.click(within(left).getByText("left-first.txt"));
    fireEvent.focus(within(left).getByRole("textbox", { name: "Folder path" }));
    await waitFor(() => expect(menu.latest()?.textEditing).toBe(true));
    expect(menu.latest()?.enabledCommands).not.toContain("copy");
    menu.send("copy");
    expect(clipboardClient.set).not.toHaveBeenCalled();
    expect(operationClient.transfer).not.toHaveBeenCalled();
    vi.mocked(menu.client.sync).mockRejectedValueOnce(new Error("menu failed"));
    fireEvent.focus(left);
    await screen.findByText(
      "The system menu could not be updated. Use the context menu or restart PaneDeck.",
    );
  });
});

it("only previews diagnostics on a native menu action and never automatically exports", async () => {
  const menu = nativeMenu();
  const { diagnosticClient } = await setup(menu.client);
  await waitFor(() => expect(menu.latest()?.enabledCommands).toContain("diagnostics"));
  expect(diagnosticClient.preview).not.toHaveBeenCalled();
  expect(diagnosticClient.export).not.toHaveBeenCalled();
  const revision = menu.latest()!.revision;
  const id = menu.send("diagnostics", revision);
  menu.send("diagnostics", revision, id);
  await screen.findByRole("dialog");
  expect(diagnosticClient.preview).toHaveBeenCalledTimes(1);
  expect(diagnosticClient.export).not.toHaveBeenCalled();
});

it("releases a native subscription that completes after unmount", async () => {
  const cleanupListener = vi.fn();
  let resolveSubscribe: ((cleanup: () => void) => void) | undefined;
  const client: DesktopMenuClient = {
    available: () => true,
    sync: vi.fn().mockResolvedValue(undefined),
    subscribe: () =>
      new Promise((resolve) => {
        resolveSubscribe = resolve;
      }),
  };
  await setup(client);
  cleanup();
  await act(async () => {
    resolveSubscribe?.(cleanupListener);
  });
  expect(cleanupListener).toHaveBeenCalledTimes(1);
  expect(client.sync).not.toHaveBeenCalled();
});
