import { DragInteraction, INTERNAL_DRAG_TYPE } from "./dragInteraction";
import {
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type DragEvent,
  type KeyboardEvent,
  type MouseEvent,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
} from "react";
import type { EntrySnapshot } from "../ipc/pathReferences";
import { useI18n } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/messages";
import type { PaneId, SortDirection, SortField } from "./directoryApi";
import {
  COLUMN_WIDTH_LIMITS,
  DEFAULT_COLUMN_WIDTHS,
  clampColumnWidth,
  columnWidthValue,
  loadColumnWidths,
  saveColumnWidths,
  tableMinWidth,
  withColumnWidth,
  type ColumnField,
  type ColumnWidths,
} from "./columnWidths";

import { entryKey, sortEntries } from "./sortEntries";
export { entryKey, sortEntries } from "./sortEntries";

import { normalizeTreeSelection, selectionAfterCollapse, type TreeRow } from "./directoryTree";

export const FILE_ROW_HEIGHT = 28;

const iconCache = new Map<string, string>();

function iconCacheKey(entry: EntrySnapshot): string {
  if (entry.kind === "directory" || entry.kind === "symlink") {
    return `${entry.kind}:${entry.reference.sessionId}:${entry.reference.entryId}`;
  }
  const dot = entry.displayName.lastIndexOf(".");
  return `file:${dot > 0 ? entry.displayName.slice(dot + 1).toLowerCase() : ""}`;
}

function FileIcon({
  entry,
  loadIcon,
}: {
  entry: EntrySnapshot;
  loadIcon?: (entry: EntrySnapshot) => Promise<string>;
}) {
  const key = iconCacheKey(entry);
  const [src, setSrc] = useState(() => iconCache.get(key) ?? "");
  useEffect(() => {
    if (iconCache.has(key)) return;
    if (!loadIcon || entry.kind === "symlink") return;
    let cancelled = false;
    void loadIcon(entry)
      .then((data) => {
        if (cancelled || !data) return;
        iconCache.set(key, data);
        setSrc(data);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [entry, key, loadIcon]);
  if (entry.kind === "symlink") return "↗";
  if (src) {
    return <img alt="" className="file-icon-image" draggable={false} src={src} />;
  }
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 20 18"
      fill="none"
      className={entry.kind === "directory" ? "file-icon-folder" : "file-icon-document"}
    >
      {entry.kind === "directory" ? (
        <path
          d="M1.5 5V3a1 1 0 0 1 1-1H8l2 3h7.5a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-15a1 1 0 0 1-1-1Z"
          fill="var(--color-folder-fill)"
          stroke="var(--color-folder-stroke)"
          strokeWidth=".8"
        />
      ) : (
        <path d="M4 1h8l4 4v12H4Zm8 0v4h4" stroke="currentColor" strokeWidth="1" />
      )}
    </svg>
  );
}
const ROW_HEIGHT = FILE_ROW_HEIGHT;
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
  /** Rendered below the fixed header when the directory has no visible entries. */
  emptyState?: ReactNode;
  /** Column widths and their persisted settings are scoped to one pane. */
  pane?: PaneId;
  treeRows?: readonly TreeRow[];
  onToggleTree?: (key: string) => void;
  onHoverExpand?: (key: string) => void;
  onRetryTree?: (key: string) => void;
  onOpen: (entry: EntrySnapshot) => void;
  loadIcon?: (entry: EntrySnapshot) => Promise<string>;
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
  emptyState,
  pane = "left",
  treeRows,
  onToggleTree,
  onRetryTree,
  onHoverExpand,
  onOpen,
  loadIcon,
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
  const [scrollLeft, setScrollLeft] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(400);
  const [scrollbarWidth, setScrollbarWidth] = useState(0);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [focusedId, setFocusedId] = useState<string | null>(null);
  const [anchorId, setAnchorId] = useState<string | null>(null);
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);
  const [invalidDropId, setInvalidDropId] = useState<string | null>(null);
  const [columnWidths, setColumnWidths] = useState<ColumnWidths>(() => loadColumnWidths(pane));
  const columnWidthsRef = useRef(columnWidths);
  const [resizingColumn, setResizingColumn] = useState<ColumnField | null>(null);
  const resizeRef = useRef<{
    field: ColumnField;
    startX: number;
    startWidth: number;
    startValue: number | null;
    currentWidth: number;
    moved: boolean;
    pointerId: number;
  } | null>(null);
  const lastColumnClick = useRef<{ field: ColumnField; at: number } | null>(null);

  const updateColumnWidth = (field: ColumnField, value: number | null) => {
    const next = withColumnWidth(columnWidthsRef.current, field, value);
    columnWidthsRef.current = next;
    setColumnWidths(next);
    return next;
  };

  const widthOrLimit = (field: ColumnField) =>
    columnWidthValue(columnWidthsRef.current, field) ?? COLUMN_WIDTH_LIMITS[field].min;

  const startResizing = (field: ColumnField, event: ReactPointerEvent<HTMLSpanElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const previous = lastColumnClick.current;
    if (previous && previous.field === field && event.timeStamp - previous.at <= 400) {
      // Some WebViews suppress compatibility mouse events after preventDefault,
      // so detect the second click on pointerdown as well as via onDoubleClick.
      lastColumnClick.current = null;
      saveColumnWidths(pane, updateColumnWidth(field, DEFAULT_COLUMN_WIDTHS[field]));
      return;
    }
    const startValue = columnWidthValue(columnWidthsRef.current, field);
    const cellWidth = event.currentTarget.parentElement?.getBoundingClientRect().width;
    const startWidth =
      field === "name" && startValue === null && Number.isFinite(cellWidth)
        ? Math.max(COLUMN_WIDTH_LIMITS.name.min, cellWidth ?? 0)
        : (startValue ?? COLUMN_WIDTH_LIMITS[field].min);
    resizeRef.current = {
      field,
      startX: event.clientX,
      startWidth,
      startValue,
      currentWidth: startWidth,
      moved: false,
      pointerId: event.pointerId,
    };
    setResizingColumn(field);
    event.currentTarget.setPointerCapture?.(event.pointerId);
  };

  const moveResizing = (event: ReactPointerEvent<HTMLSpanElement>) => {
    const drag = resizeRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const delta = event.clientX - drag.startX;
    if (delta === 0) return;
    drag.moved = true;
    drag.currentWidth = clampColumnWidth(drag.field, drag.startWidth + delta);
    updateColumnWidth(drag.field, drag.currentWidth);
  };

  const finishResizing = (commit: boolean, timeStamp?: number) => {
    const drag = resizeRef.current;
    if (!drag) return;
    resizeRef.current = null;
    setResizingColumn(null);
    if (!drag.moved) {
      if (commit) lastColumnClick.current = { field: drag.field, at: timeStamp ?? 0 };
      return;
    }
    if (!commit) {
      updateColumnWidth(drag.field, drag.startValue);
      return;
    }
    saveColumnWidths(pane, columnWidthsRef.current);
  };

  const resetColumnWidth = (field: ColumnField) => {
    saveColumnWidths(pane, updateColumnWidth(field, DEFAULT_COLUMN_WIDTHS[field]));
  };

  const resizeColumnByKeyboard = (field: ColumnField, event: KeyboardEvent<HTMLSpanElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    event.stopPropagation();
    const current =
      field === "name" && columnWidthValue(columnWidthsRef.current, "name") === null
        ? (event.currentTarget.parentElement?.getBoundingClientRect().width ??
          COLUMN_WIDTH_LIMITS.name.min)
        : widthOrLimit(field);
    const step = event.shiftKey ? 24 : 8;
    const delta = event.key === "ArrowRight" ? step : -step;
    saveColumnWidths(pane, updateColumnWidth(field, clampColumnWidth(field, current + delta)));
  };

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
  const activeDescendant =
    effectiveFocusedId && visible.some((entry) => entryKey(entry) === effectiveFocusedId)
      ? `file-row-${effectiveFocusedId}`
      : undefined;

  useEffect(() => {
    onSelectionChange?.(
      treeRows
        ? normalizeTreeSelection(treeRows, effectiveSelected)
        : sorted.filter((entry) => effectiveSelected.has(entryKey(entry))),
    );
  }, [effectiveSelected, onSelectionChange, sorted, treeRows]);

  useLayoutEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const update = () => {
      setViewportHeight(viewport.clientHeight || 400);
      const next = Math.max(0, viewport.offsetWidth - viewport.clientWidth);
      setScrollbarWidth((current) => (current === next ? current : next));
    };
    update();
    const observer =
      typeof ResizeObserver === "undefined" ? null : new ResizeObserver(() => update());
    observer?.observe(viewport);
    window.addEventListener("resize", update);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", update);
    };
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

  const columnHeader = (field: ColumnField, label: string) => {
    const value = columnWidths[field];
    return (
      <span data-column={field} key={field} role="columnheader">
        {header(field, label)}
        <span
          aria-label={t("list.resizeColumn").replace("{column}", label)}
          aria-orientation="vertical"
          aria-valuemax={COLUMN_WIDTH_LIMITS[field].max}
          aria-valuemin={COLUMN_WIDTH_LIMITS[field].min}
          aria-valuenow={value ?? undefined}
          aria-valuetext={
            value === null
              ? t("list.columnAuto").replace("{column}", label)
              : t("list.columnWidth")
                  .replace("{column}", label)
                  .replace("{value}", String(Math.round(value)))
          }
          className="column-resizer"
          onDoubleClick={(event) => {
            event.preventDefault();
            event.stopPropagation();
            resetColumnWidth(field);
          }}
          onKeyDown={(event) => resizeColumnByKeyboard(field, event)}
          onLostPointerCapture={() => finishResizing(false)}
          onPointerCancel={() => finishResizing(false)}
          onPointerDown={(event) => startResizing(field, event)}
          onPointerMove={moveResizing}
          onPointerUp={(event) => {
            finishResizing(true, event.timeStamp);
            if (event.currentTarget.hasPointerCapture?.(event.pointerId))
              event.currentTarget.releasePointerCapture(event.pointerId);
          }}
          role="separator"
          tabIndex={0}
          title={t("list.resetColumnWidth").replace("{column}", label)}
        />
      </span>
    );
  };

  const tableStyle = {
    "--col-name":
      columnWidths.name === null
        ? `minmax(${COLUMN_WIDTH_LIMITS.name.min}px, 1fr)`
        : `${columnWidths.name}px`,
    "--col-size": `${columnWidths.size}px`,
    "--col-kind": `${columnWidths.kind}px`,
    "--col-modified": `${columnWidths.modifiedAt}px`,
    "--table-min-width": `${tableMinWidth(columnWidths)}px`,
    "--table-scrollbar-width": `${scrollbarWidth}px`,
  } as CSSProperties;

  return (
    <div
      className="file-table"
      data-resizing={resizingColumn ?? undefined}
      role={treeRows ? "treegrid" : "grid"}
      style={tableStyle}
      tabIndex={treeRows ? 0 : undefined}
      onKeyDown={treeRows ? handleKeyDown : undefined}
      aria-activedescendant={treeRows ? activeDescendant : undefined}
      aria-label={t("list.label")}
      aria-multiselectable="true"
      aria-rowcount={sorted.length + 1}
      data-selected-count={effectiveSelected.size}
    >
      <div
        className="file-table-header"
        role="row"
        style={{ transform: `translateX(-${scrollLeft}px)` }}
      >
        {columnHeader("name", t("list.name"))}
        {columnHeader("size", t("list.size"))}
        {columnHeader("kind", t("list.kind"))}
        {columnHeader("modifiedAt", t("list.modified"))}
      </div>
      <div
        aria-activedescendant={!treeRows ? activeDescendant : undefined}
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
        onScroll={(event) => {
          setScrollTop(event.currentTarget.scrollTop);
          setScrollLeft(event.currentTarget.scrollLeft);
        }}
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
                        style={{
                          width: `calc(${Math.min(row.level - 1, 5)} * var(--tree-indent-size))`,
                        }}
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
                          <svg
                            aria-hidden="true"
                            width="10"
                            height="10"
                            viewBox="0 0 10 10"
                            className="tree-disclosure-icon"
                            data-expanded={row.expanded}
                          >
                            <path
                              d="m3.5 2 3 3-3 3"
                              fill="none"
                              stroke="currentColor"
                              strokeWidth="1.5"
                              strokeLinecap="round"
                              strokeLinejoin="round"
                            />
                          </svg>
                        </button>
                      ) : (
                        <span className="tree-disclosure-spacer" aria-hidden="true" />
                      )}
                      <span className="tree-file-icon" aria-hidden="true">
                        <FileIcon entry={entry} key={iconCacheKey(entry)} loadIcon={loadIcon} />
                      </span>
                    </>
                  ) : (
                    <span className="tree-file-icon" aria-hidden="true">
                      <FileIcon entry={entry} key={iconCacheKey(entry)} loadIcon={loadIcon} />
                    </span>
                  )}
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
      {emptyState && sorted.length === 0 ? (
        <div className="file-list-empty">{emptyState}</div>
      ) : null}
    </div>
  );
}
