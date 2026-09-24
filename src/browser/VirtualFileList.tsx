import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import type { EntrySnapshot } from "../ipc/pathReferences";
import { useI18n } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/messages";
import type { SortDirection, SortField } from "./directoryApi";

const ROW_HEIGHT = 32;
const OVERSCAN = 6;

interface VirtualFileListProps {
  entries: EntrySnapshot[];
  onOpen: (entry: EntrySnapshot) => void;
  onSort: (field: SortField) => void;
  sortDirection: SortDirection;
  sortField: SortField;
}

function entryKey(entry: EntrySnapshot) {
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

function formatBytes(value: string | null) {
  if (value === null) return "—";
  const bytes = Number(value);
  if (!Number.isFinite(bytes)) return value;
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

function formatDate(value: string | null) {
  if (value === null) return "—";
  const date = new Date(Number(value));
  return Number.isNaN(date.valueOf())
    ? "—"
    : date.toLocaleString([], { dateStyle: "short", timeStyle: "short" });
}

function kindMessageKey(kind: EntrySnapshot["kind"]): MessageKey {
  return `kind.${kind}`;
}

export function VirtualFileList({
  entries,
  onOpen,
  onSort,
  sortDirection,
  sortField,
}: VirtualFileListProps) {
  const { t } = useI18n();
  const viewportRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(400);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const [anchorId, setAnchorId] = useState<string | null>(null);
  const sorted = useMemo(
    () => sortEntries(entries, sortField, sortDirection),
    [entries, sortDirection, sortField],
  );
  const validIds = useMemo(() => new Set(entries.map(entryKey)), [entries]);
  const effectiveSelected = useMemo(
    () => new Set([...selected].filter((id) => validIds.has(id))),
    [selected, validIds],
  );
  const effectiveFocusedId = focusedId && validIds.has(focusedId) ? focusedId : null;
  const effectiveAnchorId = anchorId && validIds.has(anchorId) ? anchorId : null;
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const count = Math.ceil(viewportHeight / ROW_HEIGHT) + OVERSCAN * 2;
  const visible = sorted.slice(start, start + count);

  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const update = () => setViewportHeight(viewport.clientHeight || 400);
    update();
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, []);

  useEffect(() => {
    if (!effectiveFocusedId || !viewportRef.current) return;
    const index = sorted.findIndex((entry) => entryKey(entry) === effectiveFocusedId);
    if (index < 0) return;
    const top = index * ROW_HEIGHT;
    const bottom = top + ROW_HEIGHT;
    if (top < viewportRef.current.scrollTop) viewportRef.current.scrollTop = top;
    else if (bottom > viewportRef.current.scrollTop + viewportHeight)
      viewportRef.current.scrollTop = bottom - viewportHeight;
  }, [effectiveFocusedId, sortDirection, sortField, sorted, viewportHeight]);

  const selectIndex = (index: number, extend: boolean, toggle: boolean) => {
    const entry = sorted[index];
    if (!entry) return;
    const id = entryKey(entry);
    if (extend && effectiveAnchorId) {
      const anchorIndex = sorted.findIndex(
        (candidate) => entryKey(candidate) === effectiveAnchorId,
      );
      const from = Math.min(anchorIndex < 0 ? index : anchorIndex, index);
      const to = Math.max(anchorIndex < 0 ? index : anchorIndex, index);
      setSelected(new Set(sorted.slice(from, to + 1).map(entryKey)));
    } else if (toggle) {
      setSelected((current) => {
        const next = new Set(current);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      });
      setAnchorId(id);
    } else {
      setSelected(new Set([id]));
      setAnchorId(id);
    }
    setFocusedId(id);
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "a") {
      event.preventDefault();
      setSelected(new Set(sorted.map(entryKey)));
      return;
    }
    const currentIndex = effectiveFocusedId
      ? sorted.findIndex((entry) => entryKey(entry) === effectiveFocusedId)
      : -1;
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const delta = event.key === "ArrowDown" ? 1 : -1;
      const nextIndex = Math.min(sorted.length - 1, Math.max(0, currentIndex + delta));
      selectIndex(nextIndex, event.shiftKey, false);
    } else if (event.key === "Enter" && currentIndex >= 0) {
      onOpen(sorted[currentIndex]);
    }
  };

  const header = (field: SortField, label: string) => (
    <button aria-pressed={sortField === field} onClick={() => onSort(field)} type="button">
      {label}
      {sortField === field ? (sortDirection === "ascending" ? " ↑" : " ↓") : ""}
    </button>
  );

  return (
    <div
      className="file-table"
      role="grid"
      aria-label={t("list.label")}
      aria-multiselectable="true"
      data-selected-count={effectiveSelected.size}
    >
      <div className="file-table-header" role="row">
        <span role="columnheader">{header("name", t("list.name"))}</span>
        <span role="columnheader">{header("size", t("list.size"))}</span>
        <span role="columnheader">{header("kind", t("list.kind"))}</span>
        <span role="columnheader">{header("modifiedAt", t("list.modified"))}</span>
      </div>
      <div
        className="file-list-viewport"
        onKeyDown={handleKeyDown}
        onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
        ref={viewportRef}
        tabIndex={0}
      >
        <div className="file-list-spacer" style={{ height: sorted.length * ROW_HEIGHT }}>
          {visible.map((entry, visibleIndex) => {
            const index = start + visibleIndex;
            const id = entryKey(entry);
            return (
              <div
                aria-rowindex={index + 2}
                aria-selected={effectiveSelected.has(id)}
                className="file-row"
                data-focused={effectiveFocusedId === id}
                key={id}
                onClick={(event: MouseEvent) =>
                  selectIndex(index, event.shiftKey, event.metaKey || event.ctrlKey)
                }
                onDoubleClick={() => onOpen(entry)}
                role="row"
                style={{ height: ROW_HEIGHT, transform: `translateY(${index * ROW_HEIGHT}px)` }}
              >
                <span role="gridcell" title={entry.displayName}>
                  {entry.kind === "directory" ? "▸ " : ""}
                  {entry.displayName}
                </span>
                <span role="gridcell">
                  {entry.kind === "directory" ? "—" : formatBytes(entry.byteLen)}
                </span>
                <span role="gridcell">{t(kindMessageKey(entry.kind))}</span>
                <span role="gridcell">{formatDate(entry.modifiedAtUnixMillis)}</span>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}
