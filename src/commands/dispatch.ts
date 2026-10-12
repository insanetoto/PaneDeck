import type { ClipboardClient, ClipboardState, PasteDraft } from "../browser/clipboardApi";
import type { OperationClient, OperationJob } from "../browser/operationApi";
import type { PaneId } from "../browser/directoryApi";
import type { CommandContext } from "./context";

export type FileCommand =
  "copyToOther" | "moveToOther" | "copy" | "cut" | "paste" | "rename" | "newFolder" | "trash";
export type WriteCommand = "copyToOther" | "moveToOther" | "rename" | "newFolder" | "trash";

interface CommandHandlers {
  transfer: (kind: "copy" | "move", context: CommandContext) => void;
  clipboard: (intent: "copy" | "cut", context: CommandContext) => void;
  paste: (context: CommandContext) => void;
  dialog: (kind: "rename" | "newFolder" | "trash", context: CommandContext) => void;
}

export function createCommandRouter(handlers: CommandHandlers, busy: boolean) {
  return (command: FileCommand, context: CommandContext) => {
    if (!commandEnabled(command, context, busy)) return;
    switch (command) {
      case "copyToOther":
        handlers.transfer("copy", context);
        break;
      case "moveToOther":
        handlers.transfer("move", context);
        break;
      case "copy":
      case "cut":
        handlers.clipboard(command, context);
        break;
      case "paste":
        handlers.paste(context);
        break;
      case "rename":
      case "newFolder":
      case "trash":
        handlers.dialog(command, context);
        break;
    }
  };
}

export function commandEnabled(
  command: FileCommand,
  context: CommandContext,
  busy = false,
): boolean {
  if (busy) return false;
  if (command === "newFolder" && context.source === "contextMenu")
    return newFolderParentEntry(context) !== null;
  if (command === "paste" || command === "newFolder") return context.directory !== null;
  if (command === "rename") return context.entries.length === 1;
  if (command === "copyToOther" || command === "moveToOther")
    return context.entries.length > 0 && context.target !== null;
  return context.entries.length > 0;
}

/** An entry menu creates inside its captured folder; blank/menu/keyboard create at the pane root. */
export function newFolderParentEntry(context: CommandContext) {
  return context.source === "contextMenu" &&
    context.entries.length === 1 &&
    context.entries[0].kind === "directory"
    ? context.entries[0]
    : null;
}

export function dispatchWrite(
  client: OperationClient,
  command: WriteCommand,
  context: CommandContext,
  name?: string,
): Promise<OperationJob> {
  if (!commandEnabled(command, context))
    return Promise.reject(new Error("invalid command context"));
  const references = context.entries.map((entry) => entry.reference);
  if (command === "copyToOther" || command === "moveToOther") {
    const target = context.target!;
    const kind = command === "copyToOther" ? "copy" : "move";
    return target.kind === "entry"
      ? client.transferToEntry(kind, references, target.entry)
      : client.transfer(kind, references, target.directory);
  }
  if (command === "trash") return client.trash(references);
  if (!name?.trim()) return Promise.reject(new Error("empty name"));
  const parentEntry = newFolderParentEntry(context);
  if (command === "newFolder" && parentEntry)
    return client.createChildDirectory(parentEntry.reference, name);
  return command === "rename"
    ? client.rename(references[0], name)
    : client.createDirectory(context.directory!, name);
}

export function dispatchClipboard(
  client: ClipboardClient,
  intent: "copy" | "cut",
  context: CommandContext,
): Promise<ClipboardState> {
  if (!commandEnabled(intent, context)) return Promise.reject(new Error("empty selection"));
  return client.set(
    context.pane,
    intent,
    context.entries.map((entry) => entry.reference),
  );
}

export function dispatchPaste(
  client: ClipboardClient,
  context: CommandContext,
  clipboardSource?: PaneId,
): Promise<PasteDraft> {
  const other = context.target?.kind === "directory" ? context.target : null;
  // An explicit blank-area target wins. Keyboard paste retains the v1 opposite-source default.
  const preferOther = context.source !== "blankMenu" && clipboardSource === context.pane;
  const target =
    preferOther && other
      ? { pane: other.pane, directory: other.directory }
      : context.directory
        ? { pane: context.pane, directory: context.directory }
        : context.source !== "blankMenu" && other
          ? { pane: other.pane, directory: other.directory }
          : null;
  if (!target) return Promise.reject(new Error("missing paste destination"));
  return client.prepare(target.pane, target.directory);
}
