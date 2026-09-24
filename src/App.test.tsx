import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App, { FilePane } from "./App";
import type { DirectoryClient, DirectoryListing } from "./browser/directoryApi";
import type { DirectoryMonitor, DirectoryWatchReason } from "./browser/directoryMonitor";
import type { LocalLocationsClient } from "./browser/locationsApi";
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

  it("switches the active pane with Tab", () => {
    renderApp();
    const app = screen.getByRole("main", { name: "PaneDeck" });
    const right = screen.getByRole("region", { name: "Right pane" });

    fireEvent.keyDown(app, { key: "Tab" });

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
