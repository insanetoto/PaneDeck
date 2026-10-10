import type { DirectoryListing, PaneId } from "../browser/directoryApi";
import type { DirectoryReference, EntryReference, EntrySnapshot } from "../ipc/pathReferences";

export type CommandSource = "toolbar" | "keyboard" | "contextMenu" | "blankMenu" | "drag" | "menu";
export type TransferTarget =
  | { readonly kind: "directory"; readonly pane: PaneId; readonly directory: DirectoryReference }
  | { readonly kind: "entry"; readonly pane: PaneId; readonly entry: EntryReference };

/** Capture once when an action starts; never resolve its target from later focus. */
export interface CommandContext {
  readonly source: CommandSource;
  readonly pane: PaneId;
  readonly directory: DirectoryReference | null;
  readonly entries: readonly EntrySnapshot[];
  readonly target: TransferTarget | null;
}

export function captureCommandContext(
  source: CommandSource,
  pane: PaneId,
  listings: Record<PaneId, DirectoryListing | null>,
  entries: readonly EntrySnapshot[],
  target?: TransferTarget | null,
): CommandContext {
  const other: PaneId = pane === "left" ? "right" : "left";
  const destination =
    target === undefined
      ? listings[other] && {
          kind: "directory" as const,
          pane: other,
          directory: listings[other].directory,
        }
      : target;
  return Object.freeze({
    source,
    pane,
    directory: listings[pane] ? Object.freeze({ ...listings[pane].directory }) : null,
    entries: Object.freeze(
      (source === "blankMenu" ? [] : entries).map((entry) =>
        Object.freeze({ ...entry, reference: Object.freeze({ ...entry.reference }) }),
      ),
    ),
    target: destination
      ? Object.freeze(
          destination.kind === "directory"
            ? { ...destination, directory: Object.freeze({ ...destination.directory }) }
            : { ...destination, entry: Object.freeze({ ...destination.entry }) },
        )
      : null,
  });
}

export function isTextInput(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    Boolean(
      target.closest("input, textarea, select, [contenteditable]:not([contenteditable='false'])"),
    )
  );
}
