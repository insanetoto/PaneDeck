import type { PaneId } from "./directoryApi";

export type ColumnField = "name" | "size" | "kind" | "modifiedAt";
export type ColumnValue = number | null;

/** `null` means the name column shares the remaining pane width. */
export interface ColumnWidths {
  name: ColumnValue;
  size: number;
  kind: number;
  modifiedAt: number;
}

export const COLUMN_WIDTH_LIMITS: Record<ColumnField, { min: number; max: number }> = {
  name: { min: 180, max: 720 },
  size: { min: 64, max: 320 },
  kind: { min: 80, max: 320 },
  modifiedAt: { min: 120, max: 400 },
};

export const DEFAULT_COLUMN_WIDTHS: ColumnWidths = {
  name: null,
  size: 92,
  kind: 92,
  modifiedAt: 150,
};

/** Keep narrow panes usable even when every column is at its minimum. */
export const MIN_TABLE_WIDTH = 520;
export const COLUMN_WIDTHS_STORAGE_KEY = "panedeck.column-widths.v1";
const COLUMN_WIDTHS_SCHEMA_VERSION = 1;

export function clampColumnWidth(field: ColumnField, value: number): number {
  const { min, max } = COLUMN_WIDTH_LIMITS[field];
  if (!Number.isFinite(value)) return min;
  return Math.round(Math.min(max, Math.max(min, value)));
}

export function columnWidthValue(widths: ColumnWidths, field: ColumnField): ColumnValue {
  return widths[field];
}

export function withColumnWidth(
  widths: ColumnWidths,
  field: ColumnField,
  value: ColumnValue,
): ColumnWidths {
  switch (field) {
    case "name":
      return { ...widths, name: value };
    case "size":
      return { ...widths, size: value ?? widths.size };
    case "kind":
      return { ...widths, kind: value ?? widths.kind };
    case "modifiedAt":
      return { ...widths, modifiedAt: value ?? widths.modifiedAt };
  }
}

/** The table's min width follows fixed widths; the name column stays flexible on auto. */
export function tableMinWidth(widths: ColumnWidths): number {
  const name = widths.name ?? COLUMN_WIDTH_LIMITS.name.min;
  return Math.max(MIN_TABLE_WIDTH, name + widths.size + widths.kind + widths.modifiedAt);
}

function numericOrDefault(field: Exclude<ColumnField, "name">, candidate: unknown): number {
  return typeof candidate === "number" && Number.isFinite(candidate)
    ? clampColumnWidth(field, candidate)
    : DEFAULT_COLUMN_WIDTHS[field];
}

export function sanitizeColumnWidths(candidate: unknown): ColumnWidths {
  if (typeof candidate !== "object" || candidate === null) return { ...DEFAULT_COLUMN_WIDTHS };
  const input = candidate as Record<string, unknown>;
  return {
    name:
      typeof input.name === "number" && Number.isFinite(input.name)
        ? clampColumnWidth("name", input.name)
        : null,
    size: numericOrDefault("size", input.size),
    kind: numericOrDefault("kind", input.kind),
    modifiedAt: numericOrDefault("modifiedAt", input.modifiedAt),
  };
}

interface StoredRoot {
  version: number;
  panes: Record<string, unknown>;
}

function readRoot(storage: Storage): StoredRoot | null {
  try {
    const raw = storage.getItem(COLUMN_WIDTHS_STORAGE_KEY);
    if (!raw) return null;
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed !== "object" || parsed === null) return null;
    const root = parsed as { version?: unknown; panes?: unknown };
    if (
      root.version !== COLUMN_WIDTHS_SCHEMA_VERSION ||
      typeof root.panes !== "object" ||
      root.panes === null ||
      Array.isArray(root.panes)
    )
      return null;
    return {
      version: COLUMN_WIDTHS_SCHEMA_VERSION,
      panes: root.panes as Record<string, unknown>,
    };
  } catch {
    return null;
  }
}

function defaultStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function loadColumnWidths(
  pane: PaneId,
  storage: Storage | null = defaultStorage(),
): ColumnWidths {
  if (!storage) return { ...DEFAULT_COLUMN_WIDTHS };
  const root = readRoot(storage);
  return sanitizeColumnWidths(root?.panes[pane]);
}

export function saveColumnWidths(
  pane: PaneId,
  widths: ColumnWidths,
  storage: Storage | null = defaultStorage(),
): void {
  if (!storage) return;
  try {
    const root = readRoot(storage);
    const panes = { ...(root?.panes ?? {}), [pane]: widths };
    storage.setItem(
      COLUMN_WIDTHS_STORAGE_KEY,
      JSON.stringify({ version: COLUMN_WIDTHS_SCHEMA_VERSION, panes }),
    );
  } catch {
    // Column persistence must never block browsing.
  }
}
