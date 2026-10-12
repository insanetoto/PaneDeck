import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { CommandContext } from "./context";
import { commandEnabled, type FileCommand } from "./dispatch";
import type { Language } from "../i18n/I18nProvider";
import type { EntrySnapshot } from "../ipc/pathReferences";
import type { PaneId } from "../browser/directoryApi";
export type ContextCommand =
  | FileCommand
  | "open"
  | "toggleFolder"
  | "refresh"
  | "toggleHidden"
  | "operationLog"
  | "diagnostics";
export interface CapturedMenu {
  id: string;
  context: CommandContext;
  kind: "file" | "directory" | "multiple" | "blank";
  expanded: boolean;
  x: number;
  y: number;
}
export interface ContextMenuRequest {
  token: string;
  kind: CapturedMenu["kind"];
  language: Language;
  expanded: boolean;
  showHidden: boolean;
  enabledCommands: ContextCommand[];
  targetPane: PaneId | null;
  x: number;
  y: number;
}
export interface ContextMenuClient {
  available(): boolean;
  popup(request: ContextMenuRequest): Promise<void>;
  subscribe(
    handler: (event: { token: string; command: ContextCommand }) => void,
  ): Promise<() => void>;
}
export const tauriContextMenuClient: ContextMenuClient = {
  available: isTauri,
  popup: (request) => invoke("popup_context_menu", { request }),
  subscribe: (handler) =>
    listen<{ token: string; command: ContextCommand }>("context-menu-command", (event) =>
      handler(event.payload),
    ),
};
export interface ContextItem {
  command: ContextCommand;
  label: string;
  enabled: boolean;
  group: number;
  shortcut?: string;
}
let capturedId = 0;
export function captureMenu(
  context: CommandContext,
  x: number,
  y: number,
  expanded = false,
): CapturedMenu {
  const entry = context.entries[0];
  return {
    id: `${Date.now()}-${++capturedId}`,
    context,
    x,
    y,
    expanded,
    kind:
      context.source === "blankMenu"
        ? "blank"
        : context.entries.length > 1
          ? "multiple"
          : entry?.kind === "directory" || entry?.symlinkTarget === "directory"
            ? "directory"
            : "file",
  };
}
export function contextItems(menu: CapturedMenu, language: Language, busy = false): ContextItem[] {
  const chinese = language === "zh-CN";
  const target = menu.context.target?.pane;
  const labels: Record<ContextCommand, string> = {
    open: chinese ? "打开" : "Open",
    toggleFolder: chinese
      ? menu.expanded
        ? "折叠文件夹"
        : "展开文件夹"
      : menu.expanded
        ? "Collapse Folder"
        : "Expand Folder",
    copy: chinese ? "复制" : "Copy",
    cut: chinese ? "剪切" : "Cut",
    paste: chinese ? "粘贴…" : "Paste…",
    rename: chinese ? "重命名…" : "Rename…",
    newFolder: chinese ? "新建文件夹" : "New Folder",
    trash: chinese ? "移至废纸篓…" : "Move to Trash…",
    refresh: chinese ? "刷新" : "Refresh",
    toggleHidden: chinese ? "显示隐藏项目" : "Show Hidden Items",
    operationLog: chinese ? "操作日志…" : "Operation Log…",
    diagnostics: chinese ? "导出诊断…" : "Export Diagnostics…",
    copyToOther: chinese
      ? `复制到${target === "left" ? "左侧" : target === "right" ? "右侧" : "另一"}窗格`
      : `Copy to ${target === "left" ? "Left" : target === "right" ? "Right" : "Other"} Pane`,
    moveToOther: chinese
      ? `移动到${target === "left" ? "左侧" : target === "right" ? "右侧" : "另一"}窗格`
      : `Move to ${target === "left" ? "Left" : target === "right" ? "Right" : "Other"} Pane`,
  };
  const shortcuts: Partial<Record<ContextCommand, string>> = {
    open: "⌘↓",
    copy: "⌘C",
    cut: "⌘X",
    paste: "⌘V",
    rename: "F2",
    newFolder: "⇧⌘N",
    trash: "⌘⌫",
    copyToOther: "F5",
    moveToOther: "F6",
    refresh: "⌘R",
    toggleHidden: "⇧⌘.",
  };
  const groups: [ContextCommand, number][] = [];
  if (menu.kind === "directory") groups.push(["open", 0], ["toggleFolder", 0]);
  if (menu.kind === "directory") groups.push(["newFolder", 0]);
  if (menu.kind === "blank")
    groups.push(
      ["newFolder", 0],
      ["paste", 1],
      ["refresh", 2],
      ["toggleHidden", 2],
      ["operationLog", 3],
      ["diagnostics", 3],
    );
  else
    groups.push(
      ["copy", 1],
      ["cut", 1],
      ["copyToOther", 2],
      ["moveToOther", 2],
      ["rename", 3],
      ["trash", 3],
    );
  return groups.map(([command, group]) => {
    let enabled: boolean;
    if (command === "open") enabled = !busy && menu.kind === "directory";
    else if (command === "toggleFolder")
      enabled = !busy && menu.context.entries[0]?.kind === "directory";
    else if (command === "refresh" || command === "toggleHidden")
      enabled = !!menu.context.directory;
    else if (command === "operationLog" || command === "diagnostics") enabled = true;
    else
      enabled =
        commandEnabled(command, menu.context, busy) &&
        !(command === "rename" && menu.kind === "multiple");
    return {
      command,
      label: labels[command],
      enabled,
      group,
      shortcut:
        command === "newFolder" && menu.kind === "directory" ? undefined : shortcuts[command],
    };
  });
}
/** Bounded, captured native session; late or duplicate native events cannot redirect commands. */
export async function presentNativeMenu(
  client: ContextMenuClient,
  menu: CapturedMenu,
  language: Language,
  showHidden: boolean,
  busy: boolean,
  run: (command: ContextCommand) => void,
  failed: () => void,
  alive: () => boolean = () => true,
) {
  const token = menu.id;
  const enabledCommands = contextItems(menu, language, busy)
    .filter((item) => item.enabled)
    .map((item) => item.command);
  let disposed = false,
    consumed = false;
  const stop = await client.subscribe((event) => {
    if (
      !alive() ||
      disposed ||
      consumed ||
      event.token !== token ||
      !enabledCommands.includes(event.command)
    )
      return;
    consumed = true;
    run(event.command);
  });
  if (!alive()) {
    disposed = true;
    stop();
    return () => {};
  }
  void client
    .popup({
      token,
      kind: menu.kind,
      language,
      expanded: menu.expanded,
      showHidden,
      enabledCommands,
      targetPane: menu.context.target?.pane ?? null,
      x: menu.x,
      y: menu.y,
    })
    .catch(() => {
      if (!disposed && !consumed) failed();
    });
  return () => {
    disposed = true;
    stop();
  };
}
export type FolderAction = (entry: EntrySnapshot, expand: boolean) => void;
