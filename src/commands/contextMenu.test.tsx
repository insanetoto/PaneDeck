import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n/I18nProvider";
import type { CommandContext } from "./context";
import type { EntrySnapshot } from "../ipc/pathReferences";
import {
  captureMenu,
  contextItems,
  presentNativeMenu,
  type ContextCommand,
  type ContextMenuClient,
} from "./contextMenu";
import { FileContextMenu } from "./FileContextMenu";
const entry: EntrySnapshot = {
  reference: { sessionId: "1", entryId: "1" },
  kind: "directory",
  displayName: "Folder",
  displayNameIsLossy: false,
  byteLen: null,
  modifiedAtUnixMillis: null,
  isHidden: false,
  isReadOnly: false,
  symlinkTarget: null,
};
const context: CommandContext = {
  source: "contextMenu",
  pane: "left",
  directory: { sessionId: "1" },
  entries: [entry],
  target: { kind: "directory", pane: "right", directory: { sessionId: "2" } },
};
afterEach(cleanup);
describe("context menu", () => {
  it("distinguishes four contexts, symlinks and captured transfer targets", () => {
    const directory = captureMenu(context, 20, 30);
    expect(directory.kind).toBe("directory");
    expect(
      contextItems(directory, "en").find((item) => item.command === "newFolder"),
    ).toMatchObject({ label: "New Folder", enabled: true, shortcut: undefined });
    expect(
      contextItems(directory, "en").find((item) => item.command === "moveToOther")?.label,
    ).toBe("Move to Right Pane");
    const file = captureMenu({ ...context, entries: [{ ...entry, kind: "file" }] }, 20, 30);
    expect(contextItems(file, "en").some((item) => item.command === "open")).toBe(false);
    const multi = captureMenu(
      { ...context, entries: [entry, { ...entry, reference: { sessionId: "1", entryId: "2" } }] },
      20,
      30,
    );
    expect(contextItems(multi, "en").find((item) => item.command === "rename")?.enabled).toBe(
      false,
    );
    const blank = captureMenu({ ...context, source: "blankMenu", entries: [] }, 20, 30);
    expect(contextItems(blank, "en").map((item) => item.command)).toContain("paste");
    expect(contextItems(blank, "en").some((item) => item.command === "trash")).toBe(false);
    const link = captureMenu(
      { ...context, entries: [{ ...entry, kind: "symlink", symlinkTarget: "directory" }] },
      20,
      30,
    );
    expect(contextItems(link, "en").find((item) => item.command === "toggleFolder")?.enabled).toBe(
      false,
    );
  });
  it("subscribes before popup, ignores wrong tokens/disabled/duplicate actions and cleans up", async () => {
    let emit!: (event: { token: string; command: ContextCommand }) => void;
    const stop = vi.fn(),
      run = vi.fn(),
      order: string[] = [];
    const client: ContextMenuClient = {
      available: () => true,
      subscribe: vi.fn(async (handler) => {
        emit = handler;
        order.push("subscribe");
        return stop;
      }),
      popup: vi.fn(async () => {
        order.push("popup");
      }),
    };
    const menu = captureMenu(context, 20, 30);
    const dispose = await presentNativeMenu(client, menu, "en", false, false, run, vi.fn());
    expect(order).toEqual(["subscribe", "popup"]);
    emit({ token: "old", command: "copy" });
    emit({ token: menu.id, command: "paste" });
    expect(run).not.toHaveBeenCalled();
    emit({ token: menu.id, command: "copy" });
    emit({ token: menu.id, command: "copy" });
    expect(run).toHaveBeenCalledOnce();
    dispose();
    emit({ token: menu.id, command: "trash" });
    expect(run).toHaveBeenCalledOnce();
    expect(stop).toHaveBeenCalledOnce();
  });
  it("does not open a stale popup after asynchronous subscription resolves", async () => {
    let finish!: (stop: () => void) => void,
      alive = true;
    const stop = vi.fn(),
      client: ContextMenuClient = {
        available: () => true,
        popup: vi.fn(),
        subscribe: () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      };
    const pending = presentNativeMenu(
      client,
      captureMenu(context, 20, 30),
      "en",
      false,
      false,
      vi.fn(),
      vi.fn(),
      () => alive,
    );
    alive = false;
    finish(stop);
    await pending;
    expect(client.popup).not.toHaveBeenCalled();
    expect(stop).toHaveBeenCalledOnce();
  });
  it("fallback uses separators, keyboard focus, Escape and outside-click dismissal", () => {
    window.localStorage.setItem("panedeck.language", "en");
    const close = vi.fn(),
      choose = vi.fn();
    const client: ContextMenuClient = {
      available: () => false,
      popup: vi.fn(),
      subscribe: vi.fn(),
    };
    render(
      <I18nProvider>
        <FileContextMenu
          menu={captureMenu(context, 20, 30)}
          client={client}
          busy={false}
          showHidden={false}
          onChoose={choose}
          onClose={close}
        />
      </I18nProvider>,
    );
    const menu = screen.getByRole("menu");
    expect(screen.getAllByRole("separator")).toHaveLength(3);
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(document.activeElement).toHaveRole("menuitem");
    fireEvent.keyDown(menu, { key: "c", metaKey: true });
    expect(choose).toHaveBeenCalledWith("copy");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(close).toHaveBeenCalledOnce();
    fireEvent.pointerDown(document.body);
    expect(close).toHaveBeenCalledTimes(2);
  });
  it("shows an explicit fallback when native popup fails", async () => {
    const stop = vi.fn();
    const client: ContextMenuClient = {
      available: () => true,
      subscribe: vi.fn(async () => stop),
      popup: vi.fn(async () => {
        throw new Error("popup unavailable");
      }),
    };
    const rendered = render(
      <I18nProvider>
        <FileContextMenu
          menu={captureMenu(context, 20, 30)}
          client={client}
          busy={false}
          showHidden={false}
          onChoose={vi.fn()}
          onClose={vi.fn()}
        />
      </I18nProvider>,
    );
    expect(await screen.findByRole("menu")).toBeInTheDocument();
    expect(screen.getByRole("status")).toBeInTheDocument();
    rendered.unmount();
    expect(stop).toHaveBeenCalledOnce();
  });
});
