import { DragInteraction, INTERNAL_DRAG_TYPE } from "./dragInteraction";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import type { EntrySnapshot } from "../ipc/pathReferences";
import { useI18n } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/messages";
import type { SortDirection, SortField } from "./directoryApi";

import { entryKey, sortEntries } from "./sortEntries";
export { entryKey, sortEntries } from "./sortEntries";

import { normalizeTreeSelection, selectionAfterCollapse, type TreeRow } from "./directoryTree";

const ROW_HEIGHT = 32;
const OVERSCAN = 6;

export function calculateVisibleRange(
  scrollTop: number,
  viewportHeight: number,
  totalEntries: number,
) {
  const start = Math.min(
    Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN),
    Math.max(0, totalEntries - Math.ceil(viewportHeight / ROW_HEIGHT)),
  );
  const count = Math.ceil(viewportHeight / ROW_HEIGHT) + OVERSCAN * 2;
  return { start, count: Math.min(count, Math.max(0, totalEntries - start)) };
}

interface VirtualFileListProps {
  cutEntryKeys?: ReadonlySet<string>;
  entries: EntrySnapshot[];
  treeRows?: readonly TreeRow[];
  onToggleTree?: (key: string) => void;
  onHoverExpand?: (key: string) => void;
  onRetryTree?: (key: string) => void;
  onOpen: (entry: EntrySnapshot) => void;
  onEntryContextMenu?: (
    entry: EntrySnapshot,
    x: number,
    y: number,
    selection?: EntrySnapshot[],
  ) => void;
  onEntryDragEnd?: () => void;
  onEntryDragStart?: (entry: EntrySnapshot, selection?: EntrySnapshot[]) => string | void;
  dragToken?: string | null;
  validateDrop?: (entry: EntrySnapshot, copy: boolean) => Promise<boolean>;
  onEntryDrop?: (entry: EntrySnapshot, copy: boolean) => void;
  onSelectionChange?: (entries: EntrySnapshot[]) => void;
  onSort: (field: SortField) => void;
  sortDirection: SortDirection;
  sortField: SortField;
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
  cutEntryKeys = new Set(),
  entries,
  treeRows,
  onToggleTree,
  onRetryTree,
  onHoverExpand,
  onOpen,
  onEntryContextMenu,
  onEntryDragEnd,
  onEntryDragStart,
  onEntryDrop,
  dragToken = null,
  validateDrop,
  onSelectionChange,
  onSort,
  sortDirection,
  sortField,
}: VirtualFileListProps) {
  const { t } = useI18n();
  const viewportRef = useRef<HTMLDivElement>(null);
  const [interaction] = useState(() => new DragInteraction());
  const dragCallbacks = useRef({ onHoverExpand, validateDrop });
  useEffect(() => {
    dragCallbacks.current = { onHoverExpand, validateDrop };
  }, [onHoverExpand, validateDrop]);
  useEffect(() => {
    return () => interaction.clear();
  }, [interaction, dragToken, entries]);

  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(400);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const [anchorId, setAnchorId] = useState<string | null>(null);
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);
  const [invalidDropId, setInvalidDropId] = useState<string | null>(null);
  const sorted = useMemo(
    () =>
      treeRows ? treeRows.map((row) => row.entry) : sortEntries(entries, sortField, sortDirection),
    [entries, treeRows, sortDirection, sortField],
  );
  const validIds = useMemo(() => new Set(sorted.map(entryKey)), [sorted]);
  const effectiveSelected = useMemo(
    () => new Set([...selected].filter((id) => validIds.has(id))),
    [selected, validIds],
  );
  const effectiveFocusedId = focusedId && validIds.has(focusedId) ? focusedId : null;
  const effectiveAnchorId = anchorId && validIds.has(anchorId) ? anchorId : null;
  const { start, count } = calculateVisibleRange(scrollTop, viewportHeight, sorted.length);
  const visible = sorted.slice(start, start + count);

  useEffect(() => {
    onSelectionChange?.(
      treeRows
        ? normalizeTreeSelection(treeRows, effectiveSelected)
        : sorted.filter((entry) => effectiveSelected.has(entryKey(entry))),
    );
  }, [effectiveSelected, onSelectionChange, sorted, treeRows]);

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
    if (treeRows) viewportRef.current?.parentElement?.focus();
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

  const capturedSelection = (entry: EntrySnapshot) => {
    if (!effectiveSelected.has(entryKey(entry))) return [entry];
    return treeRows
      ? normalizeTreeSelection(treeRows, effectiveSelected)
      : sorted.filter((item) => effectiveSelected.has(entryKey(item)));
  };

  const toggleTree = (row: TreeRow) => {
    if (row.expanded && treeRows) {
      const next = selectionAfterCollapse(treeRows, effectiveSelected, effectiveFocusedId, row.key);
      setSelected(next.selected);
      setFocusedId(next.focused);
      setAnchorId(entryKey(row.entry));
    }
    onToggleTree?.(row.key);
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
    const row = treeRows?.[currentIndex];
    if (event.metaKey && event.key === "ArrowDown" && currentIndex >= 0) {
      event.preventDefault();
      onOpen(sorted[currentIndex]);
    } else if (row && (event.key === "ArrowRight" || event.key === "ArrowLeft") && !event.metaKey) {
      event.preventDefault();
      if (event.key === "ArrowRight") {
        if (row.entry.kind === "directory" && !row.expanded) toggleTree(row);
        else if (row.expanded && treeRows?.[currentIndex + 1]?.parentKey === row.key)
          selectIndex(currentIndex + 1, false, false);
      } else if (row.expanded) toggleTree(row);
      else if (row.parentKey) {
        const parentIndex =
          treeRows?.findIndex((candidate) => candidate.key === row.parentKey) ?? -1;
        if (parentIndex >= 0) selectIndex(parentIndex, false, false);
      }
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
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
      role={treeRows ? "treegrid" : "grid"}
      tabIndex={treeRows ? 0 : undefined}
      onKeyDown={treeRows ? handleKeyDown : undefined}
      aria-activedescendant={
        treeRows && effectiveFocusedId ? `file-row-${effectiveFocusedId}` : undefined
      }
      aria-label={t("list.label")}
      aria-multiselectable="true"
      aria-rowcount={sorted.length + 1}
      data-selected-count={effectiveSelected.size}
    >
      <div className="file-table-header" role="row">
        <span role="columnheader">{header("name", t("list.name"))}</span>
        <span role="columnheader">{header("size", t("list.size"))}</span>
        <span role="columnheader">{header("kind", t("list.kind"))}</span>
        <span role="columnheader">{header("modifiedAt", t("list.modified"))}</span>
      </div>
      <div
        aria-activedescendant={
          !treeRows && effectiveFocusedId ? `file-row-${effectiveFocusedId}` : undefined
        }
        className="file-list-viewport"
        onKeyDown={treeRows ? undefined : handleKeyDown}
        onDragOver={(event) => {
          if (!dragToken || !event.dataTransfer.types.includes(INTERNAL_DRAG_TYPE)) return;
          if (viewportRef.current) interaction.edge(viewportRef.current, event.clientY);
          if (!(event.target as HTMLElement).closest(".file-row")) interaction.clearHover();
        }}
        onDragLeave={(event) => {
          if (!event.currentTarget.contains(event.relatedTarget as Node | null))
            interaction.clear();
        }}
        onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)}
        ref={viewportRef}
        role="rowgroup"
        tabIndex={treeRows ? undefined : 0}
      >
        <div className="file-list-spacer" style={{ height: sorted.length * ROW_HEIGHT }}>
          {visible.map((entry, visibleIndex) => {
            const index = start + visibleIndex;
            const id = entryKey(entry);
            const row = treeRows?.[index];
            return (
              <div
                aria-rowindex={index + 2}
                aria-level={row?.level}
                aria-posinset={row?.position}
                aria-setsize={row?.siblings}
                aria-expanded={row && entry.kind === "directory" ? row.expanded : undefined}
                aria-busy={row?.loading}
                aria-selected={effectiveSelected.has(id)}
                className="file-row"
                data-cut={cutEntryKeys.has(id)}
                data-drop-target={Boolean(dragToken) && dropTargetId === `${dragToken}:${id}`}
                data-drop-invalid={Boolean(dragToken) && invalidDropId === `${dragToken}:${id}`}
                title={
                  dragToken && invalidDropId === `${dragToken}:${id}`
                    ? t("operation.invalidDrop")
                    : undefined
                }
                data-focused={effectiveFocusedId === id}
                draggable
                id={`file-row-${id}`}
                key={id}
                onClick={(event: MouseEvent) =>
                  selectIndex(index, event.shiftKey, event.metaKey || event.ctrlKey)
                }
                onDoubleClick={() => onOpen(entry)}
                onDragEnd={() => {
                  interaction.clear();
                  setDropTargetId(null);
                  setInvalidDropId(null);
                  onEntryDragEnd?.();
                }}
                onDragLeave={(event) => {
                  if (event.currentTarget.contains(event.relatedTarget as Node | null)) return;
                  interaction.clearHover();
                  setDropTargetId(null);
                  setInvalidDropId(null);
                }}
                onDragOver={(event: DragEvent) => {
                  if (!dragToken || !event.dataTransfer.types.includes(INTERNAL_DRAG_TYPE)) return;
                  if (viewportRef.current) interaction.edge(viewportRef.current, event.clientY);
                  event.preventDefault();
                  event.stopPropagation();
                  const directoryLike =
                    entry.kind === "directory" || entry.symlinkTarget === "directory";
                  if (!directoryLike || entry.isReadOnly) {
                    event.dataTransfer.dropEffect = "none";
                    setInvalidDropId(`${dragToken}:${id}`);
                    interaction.clearHover();
                    return;
                  }
                  const copy = event.altKey;
                  const allowed = interaction.target(
                    `${dragToken}:${id}:${copy}`,
                    () =>
                      dragCallbacks.current.validateDrop?.(entry, copy) ?? Promise.resolve(false),
                    (valid) => {
                      setDropTargetId(valid ? `${dragToken}:${id}` : null);
                      setInvalidDropId(valid ? null : `${dragToken}:${id}`);
                    },
                    row && !row.expanded && entry.kind === "directory"
                      ? () => dragCallbacks.current.onHoverExpand?.(row.key)
                      : undefined,
                  );
                  event.dataTransfer.dropEffect = allowed ? (copy ? "copy" : "move") : "none";
                }}
                onDragStart={(event) => {
                  if (!effectiveSelected.has(id)) selectIndex(index, false, false);
                  const token = onEntryDragStart?.(entry, capturedSelection(entry));
                  if (!token) {
                    event.preventDefault();
                    return;
                  }
                  event.dataTransfer.effectAllowed = "copyMove";
                  event.dataTransfer.dropEffect = "none";
                  event.dataTransfer.setData(INTERNAL_DRAG_TYPE, token);
                }}
                onDrop={(event) => {
                  if (!dragToken || event.dataTransfer.getData(INTERNAL_DRAG_TYPE) !== dragToken)
                    return;
                  event.preventDefault();
                  event.stopPropagation();
                  interaction.clear();
                  setDropTargetId(null);
                  setInvalidDropId(null);
                  onEntryDrop?.(entry, event.altKey || event.dataTransfer.dropEffect === "copy");
                }}
                onContextMenu={(event) => {
                  event.preventDefault();
                  if (!effectiveSelected.has(id)) selectIndex(index, false, false);
                  onEntryContextMenu?.(
                    entry,
                    event.clientX,
                    event.clientY,
                    capturedSelection(entry),
                  );
                }}
                role="row"
                style={{ height: ROW_HEIGHT, transform: `translateY(${index * ROW_HEIGHT}px)` }}
              >
                <span role="gridcell" className="tree-name-cell" title={entry.displayName}>
                  {row ? (
                    <>
                      <span
                        className="tree-indent"
                        aria-hidden="true"
                        style={{ width: Math.min(row.level - 1, 5) * 12 }}
                      />
                      {entry.kind === "directory" ? (
                        <button
                          className="tree-disclosure"
                          aria-label={`${t(row.expanded ? "tree.collapse" : "tree.expand")}: ${entry.displayName}`}
                          aria-expanded={row.expanded}
                          tabIndex={-1}
                          type="button"
                          onClick={(event) => {
                            event.stopPropagation();
                            toggleTree(row);
                          }}
                          onDoubleClick={(event) => event.stopPropagation()}
                        >
                          {row.expanded ? "▾" : "▸"}
                        </button>
                      ) : (
                        <span className="tree-disclosure-spacer" aria-hidden="true" />
                      )}
                      <span className="tree-file-icon" aria-hidden="true">
                        {entry.kind === "symlink" ? (
                          "↗"
                        ) : (
                          <svg width="16" height="16" viewBox="0 0 20 18" fill="none">
                            {entry.kind === "directory" ? (
                              <path d="M1 5V2h7l2 3h9v11H1Z" fill="currentColor" />
                            ) : (
                              <path
                                d="M4 1h8l4 4v12H4Zm8 0v4h4"
                                stroke="currentColor"
                                strokeWidth="1.5"
                              />
                            )}
                          </svg>
                        )}
                      </span>
                    </>
                  ) : entry.kind === "directory" ? (
                    "▸ "
                  ) : null}
                  <span className="tree-name">{entry.displayName}</span>
                  {row?.loading ? (
                    <span className="tree-loading" role="status" aria-label={t("pane.loading")}>
                      …
                    </span>
                  ) : null}
                  {row?.error || row?.watchError ? (
                    <button
                      className="tree-retry"
                      type="button"
                      aria-label={`${t("pane.retry")}: ${entry.displayName}. ${row.error ? t(`error.${row.error}`) : row.watchError === "resourceLimit" ? t("error.resourceLimit") : t("tree.watchFailed")}`}
                      title={
                        row.error
                          ? t(`error.${row.error}`)
                          : row.watchError === "resourceLimit"
                            ? t("error.resourceLimit")
                            : t("tree.watchFailed")
                      }
                      onClick={(event) => {
                        event.stopPropagation();
                        onRetryTree?.(row.key);
                      }}
                      onDoubleClick={(event) => event.stopPropagation()}
                    >
                      ↻
                    </button>
                  ) : null}
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
