import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App, { FilePane } from "./App";
import type { DirectoryClient, DirectoryListing } from "./browser/directoryApi";
import type { DirectoryMonitor, DirectoryWatchReason } from "./browser/directoryMonitor";
import type { LocalLocationsClient } from "./browser/locationsApi";
import type { ClipboardClient } from "./browser/clipboardApi";
import type { OperationClient, OperationJob } from "./browser/operationApi";
import type { DiagnosticClient } from "./browser/diagnosticApi";
import type { RecoveryClient } from "./browser/recoveryApi";
import { I18nProvider, LANGUAGE_STORAGE_KEY } from "./i18n/I18nProvider";

function renderApp() {
  const locationsClient: LocalLocationsClient = {
    list: vi.fn().mockResolvedValue({ locations: [], volumeScanFailed: false }),
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

  it("keeps pane-local state independent and marks activity with text", () => {
    renderApp();
    const left = screen.getByRole("region", { name: "Left pane" });
    const right = screen.getByRole("region", { name: "Right pane" });
    const leftHidden = within(left).getByRole("button", { name: "Show hidden items" });
    const rightHidden = within(right).getByRole("button", { name: "Show hidden items" });

    expect(within(left).getByText("Active")).toBeInTheDocument();
    expect(within(right).queryByText("Active")).not.toBeInTheDocument();

    fireEvent.click(rightHidden);
    expect(rightHidden).toHaveAttribute("aria-pressed", "true");
    expect(leftHidden).toHaveAttribute("aria-pressed", "false");
    expect(within(right).getByText("Active")).toBeInTheDocument();
  });

  it("switches the active pane with Control-Tab without stealing standard Tab navigation", () => {
    renderApp();
    const app = screen.getByRole("main", { name: "PaneDeck" });
    const right = screen.getByRole("region", { name: "Right pane" });

    fireEvent.keyDown(app, { key: "Tab" });
    expect(within(right).queryByText("Active")).not.toBeInTheDocument();

    fireEvent.keyDown(app, { key: "Tab", ctrlKey: true });

    expect(within(right).getByText("Active")).toBeInTheDocument();
    expect(right).toHaveFocus();
  });

  it("resizes panes by dragging the separator and resets on double click", () => {
    renderApp();
    const separator = screen.getByRole("separator");
    const workspace = separator.parentElement;
    if (!workspace) throw new Error("workspace is missing");
    Object.defineProperty(workspace, "getBoundingClientRect", {
      value: () => ({ left: 0, width: 1000 }),
    });

    fireEvent.pointerDown(separator, { clientX: 500, pointerId: 1 });
    fireEvent.pointerMove(separator, { clientX: 650, pointerId: 1 });
    fireEvent.pointerUp(separator, { pointerId: 1 });

    expect(separator).toHaveAttribute("aria-valuenow", "65");
    fireEvent.doubleClick(separator);
    expect(separator).toHaveAttribute("aria-valuenow", "50");
    separator.focus();
    fireEvent.keyDown(separator, { key: "ArrowRight" });
    expect(separator).toHaveAttribute("aria-valuenow", "52");
  });

  it("warns when the previous app session ended with unfinished tasks", () => {
    window.localStorage.setItem("panedeck.active-tasks.v1", "[42]");
    renderApp();
    expect(screen.getByRole("alert")).toHaveTextContent("closed with unfinished tasks");
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
      list: vi.fn().mockResolvedValue({ locations: [], volumeScanFailed: false }),
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
    fireEvent.click(screen.getByRole("button", { name: "Diagnostics" }));
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
      list: vi.fn().mockResolvedValue({ locations: [], volumeScanFailed: false }),
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
      trash: vi.fn(async () => result("trash")),
      listJobs: vi.fn(async () => []),
      cancel: vi.fn(async () => ({
        ...result("copy"),
        state: "cancellationRequested" as const,
      })),
      resolveConflict: vi.fn(async () => result("copy")),
    };
    const locationsClient: LocalLocationsClient = {
      list: vi.fn().mockResolvedValue({ locations: [], volumeScanFailed: false }),
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

    fireEvent.click(screen.getByRole("button", { name: "Copy →" }));
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
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Trash" }));
    const trashDialog = await screen.findByRole("dialog");
    fireEvent.click(within(trashDialog).getByRole("button", { name: "Confirm" }));
    await waitFor(() =>
      expect(operationClient.trash).toHaveBeenCalledWith([{ sessionId: "left", entryId: "1" }]),
    );
  });

  it("routes drag defaults and modifier copies through the operation service", async () => {
    const rightListing = listing("/destination", "folder", "right");
    rightListing.entries[0].kind = "directory";
    const directoryClient: DirectoryClient = {
      openPath: vi.fn((path: string, options) =>
        Promise.resolve(
          options.pane === "left" ? listing(path, "source.txt", "left") : rightListing,
        ),
      ),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn((directory) =>
        Promise.resolve(
          directory.sessionId === "left" ? listing("/source", "source.txt", "left") : rightListing,
        ),
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
      transfer: vi.fn(async (kind) => ({ ...queued, kind })),
      transferToEntry: vi.fn(async (kind) => ({ ...queued, kind })),
      rename: vi.fn(),
      createDirectory: vi.fn(),
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
            list: vi.fn().mockResolvedValue({ locations: [], volumeScanFailed: false }),
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
      setData: vi.fn(),
    };
    const source = await within(left).findByText("source.txt");
    await within(right).findByTitle("folder");
    fireEvent.dragStart(source.closest("[role='row']")!, { dataTransfer });
    fireEvent.dragOver(right, { dataTransfer });
    fireEvent.drop(right, { dataTransfer });
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

  it("opens local locations, disables unavailable ones, and adds a local favorite", async () => {
    const client: DirectoryClient = {
      openPath: vi.fn().mockResolvedValue(listing("/Users/test", "home.txt", "1")),
      openChild: vi.fn(),
      openParent: vi.fn(),
      reopen: vi.fn(),
    };
    const addFavorite = vi.fn();
    render(
      <I18nProvider>
        <FilePane
          active
          client={client}
          id="left"
          locations={[
            { id: "home", kind: "home", name: "Home", path: "/Users/test", available: true },
            {
              id: "downloads",
              kind: "downloads",
              name: "Downloads",
              path: "/Users/test/Downloads",
              available: false,
            },
          ]}
          onActivate={vi.fn()}
          onAddFavorite={addFavorite}
          paneRef={createRef<HTMLElement>()}
        />
      </I18nProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: "Home" }));
    expect(await screen.findByText("home.txt")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Downloads" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Add current folder to favorites" }));
    expect(addFavorite).toHaveBeenCalledWith({ name: "test", path: "/Users/test" });
  });
});
