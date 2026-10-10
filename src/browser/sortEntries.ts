import type { EntrySnapshot } from "../ipc/pathReferences";
import type { SortDirection, SortField } from "./directoryApi";
export function entryKey(entry: EntrySnapshot) {
  return `${entry.reference.sessionId}:${entry.reference.entryId}`;
}

function compareEntries(left: EntrySnapshot, right: EntrySnapshot, field: SortField) {
  if (field === "size") {
    const leftSize = BigInt(left.byteLen ?? "0");
    const rightSize = BigInt(right.byteLen ?? "0");
    return leftSize === rightSize ? 0 : leftSize < rightSize ? -1 : 1;
  }
  if (field === "modifiedAt") {
    const leftTime = BigInt(left.modifiedAtUnixMillis ?? "0");
    const rightTime = BigInt(right.modifiedAtUnixMillis ?? "0");
    return leftTime === rightTime ? 0 : leftTime < rightTime ? -1 : 1;
  }
  if (field === "kind") return left.kind.localeCompare(right.kind);
  return left.displayName.localeCompare(right.displayName, undefined, { numeric: true });
}

export function sortEntries(
  entries: readonly EntrySnapshot[],
  field: SortField,
  direction: SortDirection,
) {
  return [...entries].sort((left, right) => {
    const compared = compareEntries(left, right, field);
    const stable = compared === 0 ? entryKey(left).localeCompare(entryKey(right)) : compared;
    return direction === "ascending" ? stable : -stable;
  });
}
