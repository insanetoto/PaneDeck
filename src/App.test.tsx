import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App, { FilePane } from "./App";
import type { DirectoryClient, DirectoryListing } from "./browser/directoryApi";
import type { DirectoryMonitor, DirectoryWatchReason } from "./browser/directoryMonitor";
import type { LocalLocationsClient } from "./browser/locationsApi";
import type { ClipboardClient } from "./browser/clipboardApi";
import type { OperationClient, OperationJob, OperationSnapshot } from "./browser/operationApi";
import type { DiagnosticClient } from "./browser/diagnosticApi";
import type { RecoveryClient } from "./browser/recoveryApi";
import { I18nProvider, LANGUAGE_STORAGE_KEY } from "./i18n/I18nProvider";
import { DEFAULT_COLUMN_WIDTHS, saveColumnWidths } from "./browser/columnWidths";

function renderApp() {
  const locationsClient: LocalLocationsClient = {
    list: vi.fn().mockResolvedValue({ locations: [], warnings: [] }),
  };
  return render(
    <I18nProvider>
      <App locationsClient={locationsClient} />
    </I18nProvider>,
  );
}

describe("dual-pane shell", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "en");
  });

  afterEach(() => {
    cleanup();
    window.localStorage.clear();
  });

  it("keeps pane-local state independent without activity decorations", () => {
    renderApp();
    const left = screen.getByRole("region", { name: "Left pane" });
    const right = screen.getByRole("region", { name: "Right pane" });
    const leftHidden = within(left).getByRole("button", { name: "Show hidden items" });
    const rightHidden = within(right).getByRole("button", { name: "Show hidden items" });

    expect(left).not.toHaveAttribute("data-active");
    expect(screen.queryByText("Active")).not.toBeInTheDocument();
    expect(document.querySelector(".app-toolbar")).toBeNull();
    expect(within(left).queryByText("Left pane")).not.toBeInTheDocument();
    expect(within(right).queryByText("Right pane")).not.toBeInTheDocument();
    const icons = document.querySelectorAll<HTMLImageElement>(".pane-product-icon");
    expect(icons).toHaveLength(2);
    expect(icons[0].src).toBe(icons[1].src);
    expect(icons[0]).toHaveAttribute("alt", "");
    expect(icons[0]).not.toHaveAttribute("tabindex");
    expect(within(right).queryByText("Active")).not.toBeInTheDocument();

    fireEvent.click(rightHidden);
    expect(rightHidden).toHaveAttribute("aria-pressed", "true");
    expect(leftHidden).toHaveAttribute("aria-pressed", "false");
    expect(right).not.toHaveAttribute("data-active");
  });

  it("switches the active pane with Control-Tab without stealing standard Tab navigation", () => {
    renderApp();
    const app = screen.getByRole("main", { name: "PaneDeck" });
    const right = screen.getByRole("region", { name: "Right pane" });

    fireEvent.keyDown(app, { key: "Tab" });
    expect(within(right).queryByText("Active")).not.toBeInTheDocument();

    fireEvent.keyDown(app, { key: "Tab", ctrlKey: true });

    expect(right).not.toHaveAttribute("data-active");
    expect(right).toHaveFocus();
  });

  it("automatically prompts conflicts, resolves a batch and opens only the requested log dialog", async () => {
    let receive!: (snapshot: OperationSnapshot) => void;
    const status = { persistenceEnabled: true, failedWrites: 0, lastError: null };
    const conflict: OperationJob = {
      jobId: 9,
      operationId: "logical",
      kind: "copy",
      state: "awaitingDecision",
      itemCount: 1,
      completedItems: 0,
      failedItems: 0,
      completedUnits: null,
      totalUnits: null,
      bytesPerSecond: null,
      elapsedMillis: 0,
      itemOutcomes: [],
      canCancel: true,
      conflict: {
        kind: "fileReplacement",
        replaceAllowed: true,
        source: {
          name: "incoming",
          parentContext: "/source",
          kind: "file",
          byteLen: 1,
          modifiedMillis: null,
        },
        target: {
          name: "incoming",
          parentContext: "/target",
          kind: "file",
          byteLen: 1,
          modifiedMillis: null,
        },
      },
    };
    const done = {
      ...conflict,
      conflict: undefined,
      state: "completed" as const,
      operationState: "completed" as const,
      operationOutcomes: ["succeeded" as const],
      itemOutcomes: ["succeeded" as const],
      canCancel: false,
    };
    const client = {
      watch: vi.fn(async (callback) => {
        receive = callback;
        return vi.fn();
      }),
      resolveConflict: vi.fn(async () => done),
      getLog: vi.fn(async () => ({ events: [], status, recoveryStatus: status })),
    } as unknown as OperationClient;
    render(
      <I18nProvider>
        <App
          operationClient={client}
          locationsClient={{ list: async () => ({ locations: [], warnings: [] }) }}
        />
      </I18nProvider>,
    );
    await waitFor(() => expect(client.watch).toHaveBeenCalledOnce());
    act(() =>
      receive({ sequence: 1, jobs: [conflict], logStatus: status, recoveryLogStatus: status }),
    );
    const dialog = await screen.findByRole("dialog");
    fireEvent.click(within(dialog).getByRole("checkbox"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Keep both" }));
    await waitFor(() => expect(client.resolveConflict).toHaveBeenCalledWith(9, "keepBoth", true));
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(document.querySelector(".task-drawer")).toBeNull();
    act(() =>
      receive({
        sequence: 2,
        jobs: [
          {
            ...done,
            state: "failed",
            operationState: "failed",
            itemOutcomes: ["failed"],
            operationOutcomes: ["failed"],
          },
        ],
        logStatus: status,
        recoveryLogStatus: status,
      }),
    );
    fireEvent.click(await screen.findByRole("button", { name: "Operation log" }));
    expect(await screen.findByRole("dialog", { name: "Operation log" })).toBeInTheDocument();
    expect(client.getLog).toHaveBeenCalledOnce();
    expect(document.querySelector(".task-drawer")).toBeNull();
  });

  it("resizes panes by dragging the separator and resets on double click", () => {
    renderApp();
    const separator = screen.getByRole("separator", { name: "Resize panes" });
    const workspace = separator.parentElement;
    if (!workspace) throw new Error("workspace is missing");
    Object.defineProperty(workspace, "getBoundingClientRect", {
      value: () => ({ left: 0, width: 1000 }),
    });

    fireEvent.pointerDown(separator, { clientX: 500, pointerId: 1 });
    fireEvent.pointerMove(separator, { clientX: 650, pointerId: 1 });
    fireEvent.pointerUp(separator, { pointerId: 1 });

    expect(separator).toHaveAttribute("aria-valuenow", "65");
    expect(workspace.style.getPropertyValue("--left-pane-width")).toBe("65fr");
    expect(workspace.style.getPropertyValue("--right-pane-width")).toBe("35fr");
    fireEvent.doubleClick(separator);
    expect(separator).toHaveAttribute("aria-valuenow", "50");
    separator.focus();
    fireEvent.keyDown(separator, { key: "ArrowRight" });
    expect(separator).toHaveAttribute("aria-valuenow", "52");
  });

  it("shows a standard directory once when Finder also has it as a favorite", async () => {
    const locationsClient: LocalLocationsClient = {
      list: vi.fn().mockResolvedValue({
        locations: [
          {
            id: "desktop",
            kind: "desktop",
            source: "standard",
            name: "Desktop",
            path: "/Users/test/Desktop",
            available: true,
            isLocal: true,
            isRemovable: false,
          },
        ],
        warnings: [],
      }),
    };
    const finderFavoritesClient = {
      list: vi.fn().mockResolvedValue({
        favorites: [
          {
            id: "finder:desktop",
            name: "Desktop",
            path: "/Users/test/Desktop",
            available: true,
          },
        ],
        unresolved: 0,
        failed: false,
        timedOut: false,
      }),
    };
    render(
      <I18nProvider>
        <App finderFavoritesClient={finderFavoritesClient} locationsClient={locationsClient} />
      </I18nProvider>,
    );
    const left = screen.getByRole("region", { name: "Left pane" });
    const sidebar = within(left).getByRole("complementary", { name: "Locations" });
    await waitFor(() =>
      expect(within(sidebar).getAllByRole("button", { name: "Desktop" })).toHaveLength(1),
    );
    expect(within(sidebar).queryByRole("button", { name: "Remove favorite: Desktop" })).toBeNull();
  });

  it("warns when the previous app session ended with unfinished tasks", () => {
    window.localStorage.setItem("panedeck.active-tasks.v1", "[42]");
    renderApp();
    expect(screen.getByRole("alert")).toHaveTextContent("closed with unfinished tasks");
  });

  it("requires explicit confirmation for interrupted-task cleanup and never auto-resumes", async () => {
    const recoveryClient: RecoveryClient = {
      list: vi.fn().mockResolvedValue([
        {
          recoveryId: "owned-recovery",
          jobId: 17,
          kind: "move",
          checkpoint: "copying",
          sourceItemsPresent: 1,
          residualItems: 1,
          cleanupAvailable: true,
        },
      ]),
      confirmCleanup: vi.fn().mockResolvedValue(undefined),
    };
    const operationClient = {
      listJobs: vi.fn().mockResolvedValue([]),
      transfer: vi.fn(),
    } as unknown as OperationClient;
    render(
      <I18nProvider>
        <App
          recoveryClient={recoveryClient}
          operationClient={operationClient}
          locationsClient={{ list: async () => ({ locations: [], warnings: [] }) }}
        />
      </I18nProvider>,
    );
    const button = await screen.findByRole("button", { name: "Remove PaneDeck residue" });
    expect(recoveryClient.confirmCleanup).not.toHaveBeenCalled();
    expect(operationClient.transfer).not.toHaveBeenCalled();
    const confirm = vi
      .spyOn(window, "confirm")
      .mockReturnValueOnce(false)
      .mockReturnValueOnce(true);
    fireEvent.click(button);
    expect(recoveryClient.confirmCleanup).not.toHaveBeenCalled();
    fireEvent.click(button);
    await waitFor(() =>
      expect(recoveryClient.confirmCleanup).toHaveBeenCalledExactlyOnceWith("owned-recovery"),
    );
    confirm.mockRestore();
  });

  it("does not prepare or export diagnostics until the user asks", async () => {
    const diagnosticClient: DiagnosticClient = {
      preview: vi.fn().mockResolvedValue({ fileName: "diagnostics.json", content: "{}" }),
      export: vi.fn().mockResolvedValue(undefined),
    };
    const recoveryClient: RecoveryClient = {
      list: vi.fn().mockResolvedValue([]),
      confirmCleanup: vi.fn().mockResolvedValue(undefined),
    };
    const locationsClient: LocalLocationsClient = {
      list: vi.fn().mockResolvedValue({ locations: [], warnings: [] }),
    };
    render(
      <I18nProvider>
        <App
          diagnosticClient={diagnosticClient}
          locationsClient={locationsClient}
          recoveryClient={recoveryClient}
        />
      </I18nProvider>,
    );
    expect(diagnosticClient.preview).not.toHaveBeenCalled();
    expect(diagnosticClient.export).not.toHaveBeenCalled();
    fireEvent.contextMenu(
      screen.getByRole("region", { name: "Left pane" }).querySelector(".file-area")!,
    );
    fireEvent.click(screen.getByRole("menuitem", { name: /Diagnostics/ }));
    await screen.findByRole("dialog");
    expect(diagnosticClient.preview).toHaveBeenCalledTimes(1);
    expect(diagnosticClient.export).not.toHaveBeenCalled();
  });

  it("keeps cut declarative and visibly confirms the other pane as the paste target", async () => {
    const directoryClient: DirectoryClient = {
      openPath: vi.fn((path: string, options) =>
        Promise.resolve(
          listing(path, options.pane === "left" ? "source.txt" : "target.txt", options.pane),
        ),
      ),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn(),
    };
    const clipboardClient: ClipboardClient = {
      set: vi.fn(async (pane, intent, entries) => ({
        intent,
        sourcePane: pane,
        itemCount: entries.length,
        cutEntries: intent === "cut" ? entries : [],
        systemSynced: true,
      })),
      prepare: vi.fn(async (targetPane) => ({
        draftId: 7,
        intent: "cut" as const,
        itemCount: 1,
        targetPane,
        destinationDisplayPath: "/destination",
      })),
      confirm: vi.fn(async () => ({
        jobId: 8,
        kind: "move" as const,
        state: "queued" as const,
        itemCount: 1,
        completedItems: 1,
        failedItems: 0,
        completedUnits: null,
        totalUnits: null,
        bytesPerSecond: null,
        elapsedMillis: 0,
        itemOutcomes: ["succeeded" as const],
        canCancel: true,
      })),
      cancel: vi.fn(async () => undefined),
    };
    const locationsClient: LocalLocationsClient = {
      list: vi.fn().mockResolvedValue({ locations: [], warnings: [] }),
    };
    render(
      <I18nProvider>
        <App
          clipboardClient={clipboardClient}
          directoryClient={directoryClient}
          locationsClient={locationsClient}
        />
      </I18nProvider>,
    );
    const left = screen.getByRole("region", { name: "Left pane" });
    const right = screen.getByRole("region", { name: "Right pane" });
    for (const [pane, path] of [
      [left, "/source"],
      [right, "/destination"],
    ] as const) {
      const input = within(pane).getByRole("textbox", { name: "Folder path" });
      fireEvent.change(input, { target: { value: path } });
      fireEvent.submit(input.closest("form")!);
    }
    const source = await within(left).findByText("source.txt");
    await within(right).findByText("target.txt");
    fireEvent.click(source);
    fireEvent.keyDown(left, { key: "x", metaKey: true });
    await waitFor(() => expect(clipboardClient.set).toHaveBeenCalled());
    expect(source.closest("[role='row']")).toHaveAttribute("data-cut", "true");
    fireEvent.keyDown(left, { key: "v", metaKey: true });

    const dialog = await screen.findByRole("dialog");
    expect(dialog).toHaveTextContent("Right pane");
    expect(dialog).toHaveTextContent("/destination");
    expect(clipboardClient.prepare).toHaveBeenCalledWith("right", { sessionId: "right" });
    fireEvent.click(within(dialog).getByRole("button", { name: "Prepare task" }));
    await waitFor(() => expect(clipboardClient.confirm).toHaveBeenCalledWith(7));
  });

  it("runs the same explicit-pane operations from mouse controls and keyboard", async () => {
    const directoryClient: DirectoryClient = {
      openPath: vi.fn((path: string, options) =>
        Promise.resolve(
          listing(path, options.pane === "left" ? "source.txt" : "target.txt", options.pane),
        ),
      ),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn((directory, options) =>
        Promise.resolve(
          listing(
            directory.sessionId === "left" ? "/source" : "/destination",
            options.pane === "left" ? "source.txt" : "target.txt",
            directory.sessionId,
          ),
        ),
      ),
    };
    const result = (kind: OperationJob["kind"]): OperationJob => ({
      jobId: 1,
      kind,
      state: "queued",
      itemCount: 1,
      completedItems: 1,
      failedItems: 0,
      completedUnits: null,
      totalUnits: null,
      bytesPerSecond: null,
      elapsedMillis: 0,
      itemOutcomes: ["succeeded"],
      canCancel: true,
    });
    const operationClient: OperationClient = {
      transfer: vi.fn(async (kind) => result(kind)),
      transferToEntry: vi.fn(async (kind) => result(kind)),
      rename: vi.fn(async () => result("rename")),
      createDirectory: vi.fn(async () => result("createDirectory")),
      createChildDirectory: vi.fn(async () => result("createDirectory")),
      trash: vi.fn(async () => result("trash")),
      listJobs: vi.fn(async () => []),
      cancel: vi.fn(async () => ({
        ...result("copy"),
        state: "cancellationRequested" as const,
      })),
      resolveConflict: vi.fn(async () => result("copy")),
    };
    const locationsClient: LocalLocationsClient = {
      list: vi.fn().mockResolvedValue({ locations: [], warnings: [] }),
    };
    render(
      <I18nProvider>
        <App
          directoryClient={directoryClient}
          locationsClient={locationsClient}
          operationClient={operationClient}
        />
      </I18nProvider>,
    );
    const app = screen.getByRole("main", { name: "PaneDeck" });
    const left = screen.getByRole("region", { name: "Left pane" });
    const right = screen.getByRole("region", { name: "Right pane" });
    for (const [pane, path] of [
      [left, "/source"],
      [right, "/destination"],
    ] as const) {
      const input = within(pane).getByRole("textbox", { name: "Folder path" });
      fireEvent.change(input, { target: { value: path } });
      fireEvent.submit(input.closest("form")!);
    }
    const source = await within(left).findByText("source.txt");
    await within(right).findByText("target.txt");
    fireEvent.click(source);

    fireEvent.contextMenu(screen.getByText("source.txt"));
    fireEvent.click(screen.getByRole("menuitem", { name: /Copy to Right Pane/ }));
    await waitFor(() =>
      expect(operationClient.transfer).toHaveBeenCalledWith(
        "copy",
        [{ sessionId: "left", entryId: "1" }],
        { sessionId: "right" },
      ),
    );

    fireEvent.click(await within(left).findByText("source.txt"));
    fireEvent.keyDown(app, { key: "F6" });
    await waitFor(() =>
      expect(operationClient.transfer).toHaveBeenCalledWith("move", expect.anything(), {
        sessionId: "right",
      }),
    );

    fireEvent.click(await within(left).findByText("source.txt"));
    fireEvent.keyDown(app, { key: "F2" });
    const renameDialog = await screen.findByRole("dialog");
    const name = within(renameDialog).getByRole("textbox", { name: "Name" });
    fireEvent.change(name, { target: { value: "renamed.txt" } });
    fireEvent.submit(name.closest("form")!);
    await waitFor(() =>
      expect(operationClient.rename).toHaveBeenCalledWith(
        { sessionId: "left", entryId: "1" },
        "renamed.txt",
      ),
    );

    fireEvent.keyDown(app, { key: "n", metaKey: true, shiftKey: true });
    const newFolderDialog = await screen.findByRole("dialog");
    const folderName = within(newFolderDialog).getByRole("textbox", { name: "Name" });
    fireEvent.change(folderName, { target: { value: "Organized" } });
    fireEvent.submit(folderName.closest("form")!);
    await waitFor(() =>
      expect(operationClient.createDirectory).toHaveBeenCalledWith(
        { sessionId: "left" },
        "Organized",
      ),
    );

    fireEvent.contextMenu(await within(left).findByText("source.txt"), {
      clientX: 40,
      clientY: 50,
    });
    const menu = await screen.findByRole("menu");
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Move to Trash/ }));
    const trashDialog = await screen.findByRole("dialog");
    fireEvent.click(within(trashDialog).getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(operationClient.trash).toHaveBeenCalledWith([{ sessionId: "left", entryId: "1" }]),
    );
  });

  it("routes drag defaults and modifier copies through the operation service", async () => {
    const rightListing = listing("/destination", "folder", "right");
    rightListing.entries[0].kind = "directory";
    const leftListing = listing("/source", "source.txt", "left");
    leftListing.entries.push(
      {
        ...leftListing.entries[0],
        reference: { sessionId: "left", entryId: "2" },
        displayName: "second.txt",
      },
      {
        ...leftListing.entries[0],
        reference: { sessionId: "left", entryId: "3" },
        displayName: "local-folder",
        kind: "directory",
      },
    );
    const directoryClient: DirectoryClient = {
      openPath: vi.fn((path: string, options) =>
        Promise.resolve(
          options.pane === "left" ? { ...leftListing, displayPath: path } : rightListing,
        ),
      ),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn((directory) =>
        Promise.resolve(directory.sessionId === "left" ? leftListing : rightListing),
      ),
    };
    const queued: OperationJob = {
      jobId: 1,
      kind: "move",
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
    const operationClient: OperationClient = {
      validateTransfer: vi.fn().mockResolvedValue(undefined),
      transfer: vi.fn(async (kind) => ({ ...queued, kind })),
      transferToEntry: vi.fn(async (kind) => ({ ...queued, kind })),
      rename: vi.fn(),
      createDirectory: vi.fn(),
      createChildDirectory: vi.fn(),
      trash: vi.fn(),
      listJobs: vi.fn(async () => []),
      cancel: vi.fn(),
      resolveConflict: vi.fn(),
    };
    render(
      <I18nProvider>
        <App
          directoryClient={directoryClient}
          locationsClient={{
            list: vi.fn().mockResolvedValue({ locations: [], warnings: [] }),
          }}
          operationClient={operationClient}
        />
      </I18nProvider>,
    );
    const left = screen.getByRole("region", { name: "Left pane" });
    const right = screen.getByRole("region", { name: "Right pane" });
    for (const [pane, path] of [
      [left, "/source"],
      [right, "/destination"],
    ] as const) {
      const input = within(pane).getByRole("textbox", { name: "Folder path" });
      fireEvent.change(input, { target: { value: path } });
      fireEvent.submit(input.closest("form")!);
    }
    const dataTransfer = {
      dropEffect: "none",
      effectAllowed: "none",
      types: ["application/x-panedeck-entries"],
      payload: "",
      setData: vi.fn((_type: string, value: string) => {
        dataTransfer.payload = value;
      }),
      getData: vi.fn(() => dataTransfer.payload),
    };
    const source = await within(left).findByText("source.txt");
    await within(right).findByTitle("folder");
    fireEvent.dragStart(source.closest("[role='row']")!, { dataTransfer });
    fireEvent.dragOver(right, { dataTransfer });
    fireEvent.drop(right, { dataTransfer });
    fireEvent.dragEnd(source.closest("[role='row']")!, { dataTransfer });
    await waitFor(() =>
      expect(operationClient.transfer).toHaveBeenCalledWith(
        "move",
        [{ sessionId: "left", entryId: "1" }],
        { sessionId: "right" },
      ),
    );

    fireEvent.dragStart(source.closest("[role='row']")!, { dataTransfer });
    const folderRow = (await within(right).findByTitle("folder")).closest("[role='row']")!;
    fireEvent.dragOver(folderRow, { dataTransfer, altKey: true });
    dataTransfer.dropEffect = "copy";
    fireEvent.drop(folderRow, { dataTransfer, altKey: true });
    await waitFor(() =>
      expect(operationClient.transferToEntry).toHaveBeenCalledWith(
        "copy",
        [{ sessionId: "left", entryId: "1" }],
        { sessionId: "right", entryId: "1" },
      ),
    );
    const localFolder = within(left).getByTitle("local-folder").closest("[role='row']")!;
    const firstRow = within(left).getByText("source.txt").closest("[role='row']")!;
    const secondRow = within(left).getByText("second.txt").closest("[role='row']")!;
    fireEvent.click(firstRow);
    fireEvent.click(secondRow, { ctrlKey: true });
    fireEvent.dragStart(firstRow, { dataTransfer });
    fireEvent.drop(localFolder, { dataTransfer });
    fireEvent.dragEnd(firstRow, { dataTransfer });
    await waitFor(() =>
      expect(operationClient.transferToEntry).toHaveBeenLastCalledWith(
        "move",
        [
          { sessionId: "left", entryId: "2" },
          { sessionId: "left", entryId: "1" },
        ],
        { sessionId: "left", entryId: "3" },
      ),
    );
    const calls = vi.mocked(operationClient.transferToEntry).mock.calls.length;
    fireEvent.dragStart(firstRow, { dataTransfer });
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.drop(localFolder, { dataTransfer });
    expect(operationClient.transferToEntry).toHaveBeenCalledTimes(calls);
    fireEvent.dragStart(firstRow, { dataTransfer });
    dataTransfer.payload = "foreign-token";
    fireEvent.drop(localFolder, { dataTransfer });
    expect(operationClient.transferToEntry).toHaveBeenCalledTimes(calls);
    fireEvent.dragEnd(firstRow, { dataTransfer });
    vi.mocked(operationClient.transferToEntry).mockRejectedValue({ code: "directoryIntoItself" });
    fireEvent.dragStart(firstRow, { dataTransfer });
    fireEvent.drop(localFolder, { dataTransfer });
    await waitFor(() =>
      expect(screen.getByText("This drop target is not available.")).toBeInTheDocument(),
    );
    expect(operationClient.transferToEntry).toHaveBeenCalledTimes(calls + 1);
  });
});

function listing(path: string, name: string, sessionId: string): DirectoryListing {
  return {
    directory: { sessionId },
    displayPath: path,
    canGoUp: path !== "/",
    warnings: [],
    entries: [
      {
        reference: { sessionId, entryId: "1" },
        displayName: name,
        displayNameIsLossy: false,
        kind: "file",
        byteLen: "4",
        modifiedAtUnixMillis: "1700000000000",
        isHidden: false,
        isReadOnly: false,
        symlinkTarget: null,
      },
    ],
  };
}

function renderPane(client: DirectoryClient) {
  return render(
    <I18nProvider>
      <FilePane
        active
        client={client}
        id="left"
        onActivate={vi.fn()}
        paneRef={createRef<HTMLElement>()}
      />
    </I18nProvider>,
  );
}

describe("pane navigation", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "en");
  });

  afterEach(cleanup);

  it("keeps the current directory and history intact when navigation fails", async () => {
    const client: DirectoryClient = {
      openPath: vi
        .fn()
        .mockResolvedValueOnce(listing("/one", "one.txt", "1"))
        .mockRejectedValueOnce({ code: "notFound" }),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn(),
    };
    renderPane(client);
    const input = screen.getByRole("textbox", { name: "Folder path" });
    fireEvent.change(input, { target: { value: "/one" } });
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByText("one.txt")).toBeInTheDocument();

    fireEvent.change(input, { target: { value: "/missing" } });
    fireEvent.submit(input.closest("form")!);

    expect(await screen.findByRole("alert")).toHaveTextContent("no longer exists");
    expect(screen.getByText("one.txt")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Back" })).toBeDisabled();
  });

  it("prompts for Finder permission only when refreshing without visibility", () => {
    const client: DirectoryClient = {
      openPath: vi.fn(),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn(),
    };
    const refresh = vi.fn();
    const openSettings = vi.fn();
    render(
      <I18nProvider>
        <FilePane
          active
          client={client}
          finderVisibilityAvailable={false}
          id="left"
          onActivate={vi.fn()}
          onOpenFinderSettings={openSettings}
          onRefreshFavorites={refresh}
          paneRef={createRef<HTMLElement>()}
        />
      </I18nProvider>,
    );

    expect(screen.queryByText(/Finder visibility is unavailable/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Refresh favorites" }));
    expect(refresh).toHaveBeenCalledOnce();
    expect(screen.getByRole("dialog")).toHaveTextContent("Finder favorites need permission");
    fireEvent.click(screen.getByRole("button", { name: "Open System Settings" }));
    expect(openSettings).toHaveBeenCalledOnce();
    expect(screen.queryByRole("dialog")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Refresh favorites" }));
    fireEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("keeps per-pane column widths when locations refresh", async () => {
    saveColumnWidths("left", { ...DEFAULT_COLUMN_WIDTHS, size: 150 });
    const client: DirectoryClient = {
      openPath: vi.fn().mockResolvedValue(listing("/one", "one.txt", "1")),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn(),
    };
    const locations = [
      {
        id: "home",
        kind: "home" as const,
        source: "standard" as const,
        name: "Home",
        path: "/Users/test",
        available: true,
        isLocal: true,
        isRemovable: false,
      },
    ];
    const pane = (items: typeof locations) => (
      <I18nProvider>
        <FilePane
          active
          client={client}
          id="left"
          locations={items}
          onActivate={vi.fn()}
          paneRef={createRef<HTMLElement>()}
        />
      </I18nProvider>
    );
    const view = render(pane([]));
    const input = screen.getByRole("textbox", { name: "Folder path" });
    fireEvent.change(input, { target: { value: "/one" } });
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByText("one.txt")).toBeInTheDocument();
    const table = view.container.querySelector<HTMLElement>(".file-table")!;
    expect(table.style.getPropertyValue("--col-size")).toBe("150px");

    view.rerender(pane(locations));
    expect(table.style.getPropertyValue("--col-size")).toBe("150px");
    expect(screen.getByText("one.txt")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Home" })).toBeInTheDocument();
  });

  it("keeps the column header adjustable in an empty folder", async () => {
    const client: DirectoryClient = {
      openPath: vi.fn().mockResolvedValue({
        directory: { sessionId: "empty" },
        displayPath: "/empty",
        canGoUp: true,
        warnings: [],
        entries: [],
      }),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn(),
    };
    const view = renderPane(client);
    const input = screen.getByRole("textbox", { name: "Folder path" });
    fireEvent.change(input, { target: { value: "/empty" } });
    fireEvent.submit(input.closest("form")!);

    expect(await screen.findByText("This folder is empty")).toBeInTheDocument();
    const size = screen.getByRole("separator", { name: "Resize Size column" });
    fireEvent.pointerDown(size, { button: 0, clientX: 100, pointerId: 2 });
    fireEvent.pointerMove(size, { clientX: 140, pointerId: 2 });
    fireEvent.pointerUp(size, { pointerId: 2 });
    expect(
      view.container
        .querySelector<HTMLElement>(".file-table")!
        .style.getPropertyValue("--col-size"),
    ).toBe("132px");
    expect(screen.getByText("This folder is empty")).toBeInTheDocument();
  });

  it("supports back, forward, parent button and keyboard parent navigation", async () => {
    const client: DirectoryClient = {
      openPath: vi
        .fn()
        .mockResolvedValueOnce(listing("/one", "one.txt", "1"))
        .mockResolvedValueOnce(listing("/two", "two.txt", "2")),
      openChild: vi.fn(),
      openParent: vi
        .fn()
        .mockResolvedValueOnce(listing("/parent", "parent.txt", "3"))
        .mockResolvedValueOnce(listing("/", "root.txt", "4")),
      reopen: vi
        .fn()
        .mockResolvedValueOnce(listing("/one", "one.txt", "5"))
        .mockResolvedValueOnce(listing("/two", "two.txt", "6")),
    };
    renderPane(client);
    const input = screen.getByRole("textbox", { name: "Folder path" });
    for (const path of ["/one", "/two"]) {
      fireEvent.change(input, { target: { value: path } });
      fireEvent.submit(input.closest("form")!);
      await screen.findByText(path === "/one" ? "one.txt" : "two.txt");
    }

    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(await screen.findByText("one.txt")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Forward" }));
    expect(await screen.findByText("two.txt")).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("region", { name: "Left pane" }), {
      key: "ArrowUp",
      metaKey: true,
    });
    expect(await screen.findByText("parent.txt")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Parent folder" }));
    expect(await screen.findByText("root.txt")).toBeInTheDocument();
    await waitFor(() => expect(client.openParent).toHaveBeenCalledTimes(2));
  });

  it("rescans after a debounced watcher signal and releases the watch on close", async () => {
    let signal: ((reason: DirectoryWatchReason) => void) | undefined;
    const stop = vi.fn();
    const monitor: DirectoryMonitor = {
      start: vi.fn(async (_pane, _directory, onRescan) => {
        signal = onRescan;
        return stop;
      }),
    };
    const client: DirectoryClient = {
      openPath: vi.fn().mockResolvedValue(listing("/watched", "before.txt", "1")),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn().mockResolvedValue(listing("/watched", "after.txt", "2")),
    };
    const rendered = render(
      <I18nProvider>
        <FilePane
          active
          client={client}
          id="left"
          monitor={monitor}
          onActivate={vi.fn()}
          paneRef={createRef<HTMLElement>()}
        />
      </I18nProvider>,
    );
    const input = screen.getByRole("textbox", { name: "Folder path" });
    fireEvent.change(input, { target: { value: "/watched" } });
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByText("before.txt")).toBeInTheDocument();
    await waitFor(() => expect(monitor.start).toHaveBeenCalledTimes(1));

    signal?.("changed");
    expect(await screen.findByText("after.txt")).toBeInTheDocument();
    expect(client.reopen).toHaveBeenCalledTimes(1);

    rendered.unmount();
    expect(stop).toHaveBeenCalled();
  });

  it("selects locations without reading, opens on double click or Enter, and keeps favorites local", async () => {
    const client: DirectoryClient = {
      openPath: vi.fn((path: string) =>
        Promise.resolve(listing(path, path.endsWith("iCloud") ? "icloud.txt" : "home.txt", "1")),
      ),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn(),
    };
    const addFavorite = vi.fn();
    const refreshLocations = vi.fn();
    const refreshFavorites = vi.fn();
    const onActivate = vi.fn();
    render(
      <I18nProvider>
        <FilePane
          active
          client={client}
          favorites={[
            { name: "system", path: "/Users/system", source: "finder", available: true },
            { name: "other", path: "/Users/other", source: "app", available: true },
          ]}
          id="left"
          locations={[
            {
              id: "home",
              kind: "home",
              source: "standard",
              name: "Home",
              path: "/Users/test",
              available: true,
              isLocal: true,
              isRemovable: false,
            },
            {
              id: "desktop",
              kind: "desktop",
              source: "standard",
              name: "Desktop",
              path: "/Users/test/Desktop",
              available: true,
              isLocal: true,
              isRemovable: false,
            },
            {
              id: "downloads",
              kind: "downloads",
              source: "standard",
              name: "Downloads",
              path: "/Users/test/Downloads",
              available: false,
              isLocal: true,
              isRemovable: false,
            },
            {
              id: "icloud-drive",
              kind: "iCloudDrive",
              source: "iCloud",
              name: "iCloud Drive",
              path: "/Users/test/iCloud",
              available: true,
              isLocal: true,
              isRemovable: false,
            },
          ]}
          onActivate={onActivate}
          onAddFavorite={addFavorite}
          finderUnresolved={1}
          onRefreshFavorites={refreshFavorites}
          onRefreshLocations={refreshLocations}
          paneRef={createRef<HTMLElement>()}
        />
      </I18nProvider>,
    );

    const home = screen.getByRole("button", { name: "Home" });
    fireEvent.click(home);
    expect(home).toHaveAttribute("aria-pressed", "true");
    expect(onActivate).toHaveBeenCalledWith("left");
    expect(client.openPath).not.toHaveBeenCalled();
    fireEvent.doubleClick(home);
    expect(await screen.findByText("home.txt")).toBeInTheDocument();
    expect(client.openPath).toHaveBeenCalledWith(
      "/Users/test",
      expect.objectContaining({ pane: "left" }),
    );

    const desktop = screen.getByRole("button", { name: "Desktop" });
    fireEvent.click(desktop);
    expect(client.openPath).toHaveBeenLastCalledWith(
      "/Users/test/Desktop",
      expect.objectContaining({ pane: "left" }),
    );

    const downloads = screen.getByRole("button", { name: "Downloads" });
    expect(downloads).not.toBeDisabled();
    expect(downloads).toHaveAttribute("data-unavailable", "true");
    fireEvent.click(downloads);
    expect(downloads).toHaveAttribute("aria-pressed", "true");

    const icloud = screen.getByRole("button", { name: "iCloud Drive" });
    fireEvent.click(icloud);
    expect(client.openPath).not.toHaveBeenCalledWith("/Users/test/iCloud", expect.anything());
    fireEvent.keyDown(icloud, { key: "Enter" });
    expect(client.openPath).toHaveBeenLastCalledWith(
      "/Users/test/iCloud",
      expect.objectContaining({ pane: "left" }),
    );
    expect(await screen.findByText("icloud.txt")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Add current folder to favorites" }));
    expect(addFavorite).toHaveBeenCalledWith({ name: "iCloud", path: "/Users/test/iCloud" });

    const sidebar = screen.getByRole("complementary", { name: "Locations" });
    const favorite = within(sidebar).getByRole("button", { name: "other" });
    fireEvent.click(favorite);
    expect(favorite).toHaveAttribute("aria-pressed", "true");
    fireEvent.doubleClick(favorite);
    expect(client.openPath).toHaveBeenLastCalledWith(
      "/Users/other",
      expect.objectContaining({ pane: "left" }),
    );

    const system = within(sidebar).getByRole("button", { name: "system" });
    expect(within(sidebar).queryByRole("button", { name: "Remove favorite: system" })).toBeNull();
    fireEvent.doubleClick(system);
    expect(client.openPath).toHaveBeenLastCalledWith(
      "/Users/system",
      expect.objectContaining({ pane: "left" }),
    );
    expect(within(sidebar).getByText("Unresolved Finder favorites: 1")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Refresh locations" }));
    expect(refreshLocations).toHaveBeenCalledOnce();
    fireEvent.click(within(sidebar).getByRole("button", { name: "Refresh favorites" }));
    expect(refreshFavorites).toHaveBeenCalledOnce();
  });
});
