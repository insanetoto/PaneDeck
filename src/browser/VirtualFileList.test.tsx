import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n/I18nProvider";
import type { EntrySnapshot } from "../ipc/pathReferences";
import type { PaneId, SortDirection, SortField } from "./directoryApi";
import { VirtualFileList, FILE_ROW_HEIGHT, calculateVisibleRange } from "./VirtualFileList";
import { loadColumnWidths } from "./columnWidths";
import tokens from "../styles/tokens.css?raw";

function entries(count: number): EntrySnapshot[] {
  return Array.from({ length: count }, (_, index) => ({
    reference: { sessionId: "1", entryId: String(index + 1) },
    displayName: `item-${String(index).padStart(5, "0")}`,
    displayNameIsLossy: false,
    kind: index % 5 === 0 ? "directory" : "file",
    byteLen: String(count - index),
    modifiedAtUnixMillis: String(1_700_000_000_000 + index),
    isHidden: false,
    isReadOnly: false,
    symlinkTarget: null,
  }));
}

function Harness({
  items,
  loadIcon,
}: {
  items: EntrySnapshot[];
  loadIcon?: (entry: EntrySnapshot) => Promise<string>;
}) {
  const [field, setField] = useState<SortField>("name");
  const [direction, setDirection] = useState<SortDirection>("ascending");
  return (
    <I18nProvider>
      <VirtualFileList
        entries={items}
        loadIcon={loadIcon}
        onOpen={vi.fn()}
        onSort={(next) => {
          setDirection(next === field && direction === "ascending" ? "descending" : "ascending");
          setField(next);
        }}
        sortDirection={direction}
        sortField={field}
      />
    </I18nProvider>
  );
}

describe("virtual file list", () => {
  afterEach(cleanup);

  it("never exposes an active descendant whose virtual row was unmounted", () => {
    const view = render(<Harness items={entries(10_000)} />);
    const viewport = view.container.querySelector<HTMLElement>(".file-list-viewport")!;
    fireEvent.keyDown(viewport, { key: "ArrowDown" });
    const id = viewport.getAttribute("aria-activedescendant")!;
    expect(document.getElementById(id)).not.toBeNull();
    viewport.scrollTop = 240_000;
    fireEvent.scroll(viewport);
    expect(document.getElementById(id)).toBeNull();
    expect(viewport).not.toHaveAttribute("aria-activedescendant");
    expect(screen.getAllByRole("row").length).toBeLessThan(40);
  });

  it("renders only a bounded window for ten thousand entries", () => {
    render(<Harness items={entries(10_000)} />);
    expect(screen.getAllByRole("row").length).toBeLessThan(40);
    expect(screen.getByRole("grid")).toHaveAttribute("data-selected-count", "0");
  });

  it("renders the native icon returned by the loader", async () => {
    const data = "data:image/png;base64,iVBORw0KGgo=";
    const loadIcon = vi.fn(async () => data);
    const { container } = render(<Harness items={entries(1)} loadIcon={loadIcon} />);
    await waitFor(() => expect(container.querySelector("img.file-icon-image")).toBeTruthy());
    expect(container.querySelector("img.file-icon-image")).toHaveAttribute("src", data);
  });

  it("keeps selection attached to entry identity after sorting", () => {
    render(<Harness items={entries(100)} />);
    const first = screen.getByTitle("item-00000").closest('[role="row"]');
    if (!first) throw new Error("first row missing");
    fireEvent.click(first);
    expect(first).toHaveAttribute("aria-selected", "true");

    fireEvent.click(screen.getByRole("button", { name: "Size" }));
    fireEvent.click(screen.getByRole("button", { name: /Size/ }));

    const sameEntry = screen.getByTitle("item-00000").closest('[role="row"]');
    expect(sameEntry).toHaveAttribute("aria-selected", "true");
  });

  it("matches mouse range selection and keyboard selection semantics", () => {
    const { container } = render(<Harness items={entries(100)} />);
    const rows = screen.getAllByRole("row").slice(1);
    fireEvent.click(rows[0]);
    fireEvent.click(rows[4], { shiftKey: true });
    expect(screen.getByRole("grid")).toHaveAttribute("data-selected-count", "5");
    fireEvent.click(rows[6], { metaKey: true });
    expect(screen.getByRole("grid")).toHaveAttribute("data-selected-count", "6");

    const viewport = container.querySelector(".file-list-viewport");
    if (!viewport) throw new Error("viewport missing");
    fireEvent.keyDown(viewport, { key: "a", metaKey: true });
    expect(screen.getByRole("grid")).toHaveAttribute("data-selected-count", "100");
    fireEvent.keyDown(viewport, { key: "ArrowDown" });
    expect(container.querySelector('[data-focused="true"]')).toBeInTheDocument();
  });
});

function TreeHarness({
  count = 4,
  onOpen = vi.fn(),
}: {
  count?: number;
  onOpen?: (entry: EntrySnapshot) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const items = entries(count);
  const rows = items
    .filter((_, index) => expanded || index !== 1)
    .map((entry, index) => ({
      entry,
      key: String(index === 0 ? 0 : expanded ? index : index + 1),
      parentKey: expanded && index === 1 ? "0" : null,
      level: expanded && index === 1 ? 2 : 1,
      position: index + 1,
      siblings: count,
      expanded: index === 0 && expanded,
      loading: false,
      error: null,
    }));
  return (
    <I18nProvider>
      <VirtualFileList
        entries={items}
        treeRows={rows}
        onToggleTree={() => setExpanded((value) => !value)}
        onOpen={onOpen}
        onSort={vi.fn()}
        sortField="name"
        sortDirection="ascending"
      />
    </I18nProvider>
  );
}

describe("tree virtual list", () => {
  afterEach(cleanup);
  it("disclosure expands without selecting or navigating; row click only selects", () => {
    const open = vi.fn();
    render(<TreeHarness onOpen={open} />);
    const grid = screen.getByRole("treegrid");
    fireEvent.click(screen.getByRole("button", { name: "Expand: item-00000" }));
    expect(grid).toHaveAttribute("data-selected-count", "0");
    const child = screen.getByTitle("item-00001").closest('[role="row"]')!;
    expect(child).toHaveAttribute("aria-level", "2");
    fireEvent.click(child);
    expect(open).not.toHaveBeenCalled();
    fireEvent.doubleClick(child);
    expect(open).toHaveBeenCalledTimes(1);
  });
  it("uses visible-row arrow navigation, collapses hidden selection and restores parent focus", () => {
    const open = vi.fn();
    render(<TreeHarness onOpen={open} />);
    const grid = screen.getByRole("treegrid");
    fireEvent.click(screen.getByTitle("item-00000").closest('[role="row"]')!);
    fireEvent.keyDown(grid, { key: "ArrowRight" });
    fireEvent.keyDown(grid, { key: "ArrowDown" });
    expect(grid).toHaveAttribute("aria-activedescendant", "file-row-1:2");
    fireEvent.click(screen.getByRole("button", { name: "Collapse: item-00000" }));
    expect(grid).toHaveAttribute("aria-activedescendant", "file-row-1:1");
    expect(grid).toHaveAttribute("data-selected-count", "0");
    fireEvent.keyDown(grid, { key: "ArrowDown", metaKey: true });
    expect(open).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(grid, { key: "ArrowRight" });
    expect(screen.getByTitle("item-00001").closest('[role="row"]')).toHaveAttribute(
      "aria-selected",
      "false",
    );
  });
  it("bounds DOM nodes for ten thousand tree rows and clamps the window after collapse", () => {
    const { container } = render(<TreeHarness count={10_000} />);
    fireEvent.click(container.querySelector(".tree-disclosure")!);
    expect(screen.getByRole("treegrid")).toHaveAttribute("aria-rowcount", "10001");
    expect(screen.getAllByRole("row").length).toBeLessThan(40);
    const viewport = container.querySelector(".file-list-viewport")!;
    fireEvent.scroll(viewport, { target: { scrollTop: FILE_ROW_HEIGHT * 9_999 } });
    expect(screen.getAllByRole("row").length).toBeGreaterThan(1);
    expect(screen.getAllByRole("row").length).toBeLessThan(40);
    const renderedRows = container.querySelectorAll<HTMLElement>(".file-row");
    expect(renderedRows[0].style.height).toBe(`${FILE_ROW_HEIGHT}px`);
    const range = calculateVisibleRange(FILE_ROW_HEIGHT * 9_999, 400, 10_000);
    expect(renderedRows[0].style.transform).toBe(`translateY(${range.start * FILE_ROW_HEIGHT}px)`);
    expect(renderedRows[renderedRows.length - 1]?.style.transform).toBe(
      `translateY(${9_999 * FILE_ROW_HEIGHT}px)`,
    );
  });
  it("resizes columns from the header without changing tree selection or expansion", () => {
    window.localStorage.clear();
    const { container } = render(<TreeHarness />);
    const grid = screen.getByRole("treegrid");
    const first = screen.getByTitle("item-00000").closest('[role="row"]')!;
    fireEvent.click(first);
    expect(grid).toHaveAttribute("data-selected-count", "1");

    const size = screen.getByRole("separator", { name: "Resize Size column" });
    size.focus();
    fireEvent.keyDown(size, { key: "ArrowRight" });

    expect(grid).toHaveAttribute("data-selected-count", "1");
    expect(screen.queryByRole("button", { name: "Collapse: item-00000" })).not.toBeInTheDocument();
    expect(
      container.querySelector<HTMLElement>(".file-table")!.style.getPropertyValue("--col-size"),
    ).toBe("100px");
  });
});

it("expands a valid directory only after hover and cancels on drag session end", async () => {
  vi.useFakeTimers();
  const items = entries(1),
    expand = vi.fn(),
    validate = vi.fn(async () => true);
  const rows = [
    {
      entry: items[0],
      key: "folder",
      parentKey: null,
      level: 1,
      position: 1,
      siblings: 1,
      expanded: false,
      loading: false,
      error: null,
    },
  ];
  const props = {
    entries: items,
    treeRows: rows,
    dragToken: "owned",
    validateDrop: validate,
    onHoverExpand: expand,
    onOpen: vi.fn(),
    onSort: vi.fn(),
    sortField: "name" as const,
    sortDirection: "ascending" as const,
  };
  const view = render(
    <I18nProvider>
      <VirtualFileList {...props} />
    </I18nProvider>,
  );
  const row = screen.getByTitle("item-00000").closest("[role='row']")!;
  const dataTransfer = { types: ["application/x-panedeck-entries"], dropEffect: "none" };
  await act(async () => {
    fireEvent.dragOver(row, { dataTransfer });
  });
  act(() => vi.advanceTimersByTime(699));
  expect(expand).not.toHaveBeenCalled();
  act(() => vi.advanceTimersByTime(1));
  expect(expand).toHaveBeenCalledExactlyOnceWith("folder");
  fireEvent.dragLeave(row);
  await act(async () => {
    fireEvent.dragOver(row, { dataTransfer });
  });
  view.rerender(
    <I18nProvider>
      <VirtualFileList {...props} dragToken={null} />
    </I18nProvider>,
  );
  act(() => vi.advanceTimersByTime(1000));
  expect(expand).toHaveBeenCalledTimes(1);
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
  vi.useRealTimers();
});

function renderList(
  options: { emptyState?: ReactNode; items?: EntrySnapshot[]; pane?: PaneId } = {},
) {
  const items = options.items ?? entries(10);
  return render(
    <I18nProvider>
      <VirtualFileList
        emptyState={options.emptyState}
        entries={items}
        pane={options.pane ?? "left"}
        onOpen={vi.fn()}
        onSort={vi.fn()}
        sortField="name"
        sortDirection="ascending"
      />
    </I18nProvider>,
  );
}

describe("column resizing", () => {
  beforeEach(() => window.localStorage.clear());
  afterEach(cleanup);

  it("exposes four named separators with current values and limits", () => {
    renderList();
    const name = screen.getByRole("separator", { name: "Resize Name column" });
    expect(name).toHaveAttribute("aria-orientation", "vertical");
    expect(name).toHaveAttribute("aria-valuemin", "180");
    expect(name).toHaveAttribute("aria-valuemax", "720");
    expect(name).toHaveAttribute("aria-valuetext", "Name column width: automatic");
    expect(name).not.toHaveAttribute("aria-valuenow");

    const size = screen.getByRole("separator", { name: "Resize Size column" });
    expect(size).toHaveAttribute("aria-valuenow", "92");
    expect(size).toHaveAttribute("aria-valuetext", "Size column width: 92 pixels");
    expect(screen.getAllByRole("separator")).toHaveLength(4);
  });

  it("resizes a column with pointer capture, saves it and clamps the drag", () => {
    const { container } = renderList();
    const table = container.querySelector<HTMLElement>(".file-table")!;
    expect(table.style.getPropertyValue("--col-size")).toBe("92px");

    const size = screen.getByRole("separator", { name: "Resize Size column" });
    fireEvent.pointerDown(size, { button: 0, clientX: 200, pointerId: 4 });
    fireEvent.pointerMove(size, { clientX: 260, pointerId: 4 });
    fireEvent.pointerUp(size, { pointerId: 4 });

    expect(table.style.getPropertyValue("--col-size")).toBe("152px");
    expect(size).toHaveAttribute("aria-valuenow", "152");
    expect(loadColumnWidths("left").size).toBe(152);

    fireEvent.pointerDown(size, { button: 0, clientX: 200, pointerId: 5 });
    fireEvent.pointerMove(size, { clientX: -400, pointerId: 5 });
    fireEvent.pointerUp(size, { pointerId: 5 });
    expect(table.style.getPropertyValue("--col-size")).toBe("64px");
  });

  it("restores the pre-drag value when the pointer is cancelled", () => {
    const { container } = renderList();
    const table = container.querySelector<HTMLElement>(".file-table")!;
    const kind = screen.getByRole("separator", { name: "Resize Kind column" });
    fireEvent.pointerDown(kind, { button: 0, clientX: 100, pointerId: 6 });
    fireEvent.pointerMove(kind, { clientX: 200, pointerId: 6 });
    expect(table.style.getPropertyValue("--col-kind")).toBe("192px");
    fireEvent.pointerCancel(kind, { pointerId: 6 });
    expect(table.style.getPropertyValue("--col-kind")).toBe("92px");
    expect(loadColumnWidths("left").kind).toBe(92);
  });

  it("adjusts by keyboard and resets a single column on double click", () => {
    const { container } = renderList();
    const table = container.querySelector<HTMLElement>(".file-table")!;
    const modified = screen.getByRole("separator", { name: "Resize Modified column" });

    fireEvent.keyDown(modified, { key: "ArrowRight" });
    expect(table.style.getPropertyValue("--col-modified")).toBe("158px");
    fireEvent.keyDown(modified, { key: "ArrowRight", shiftKey: true });
    expect(table.style.getPropertyValue("--col-modified")).toBe("182px");
    fireEvent.keyDown(modified, { key: "ArrowLeft" });
    expect(table.style.getPropertyValue("--col-modified")).toBe("174px");
    fireEvent.doubleClick(modified);
    expect(table.style.getPropertyValue("--col-modified")).toBe("150px");
    expect(loadColumnWidths("left").modifiedAt).toBe(150);
  });

  it("resets on a pointer double click when a WebView suppresses dblclick", () => {
    const { container } = renderList();
    const table = container.querySelector<HTMLElement>(".file-table")!;
    const size = screen.getByRole("separator", { name: "Resize Size column" });
    fireEvent.pointerDown(size, { button: 0, clientX: 100, pointerId: 12 });
    fireEvent.pointerMove(size, { clientX: 150, pointerId: 12 });
    fireEvent.pointerUp(size, { pointerId: 12 });
    expect(table.style.getPropertyValue("--col-size")).toBe("142px");

    fireEvent.pointerDown(size, { button: 0, clientX: 100, pointerId: 13 });
    fireEvent.pointerUp(size, { pointerId: 13 });
    fireEvent.pointerDown(size, { button: 0, clientX: 100, pointerId: 14 });
    fireEvent.pointerUp(size, { pointerId: 14 });
    expect(table.style.getPropertyValue("--col-size")).toBe("92px");
    expect(loadColumnWidths("left").size).toBe(92);
  });

  it("turns an automatic name column into a fixed width and resets it back", () => {
    const { container } = renderList();
    const table = container.querySelector<HTMLElement>(".file-table")!;
    const name = screen.getByRole("separator", { name: "Resize Name column" });
    expect(table.style.getPropertyValue("--col-name")).toBe("minmax(180px, 1fr)");

    fireEvent.pointerDown(name, { button: 0, clientX: 100, pointerId: 7 });
    fireEvent.pointerMove(name, { clientX: 140, pointerId: 7 });
    fireEvent.pointerUp(name, { pointerId: 7 });
    expect(table.style.getPropertyValue("--col-name")).toBe("220px");

    fireEvent.doubleClick(name);
    expect(table.style.getPropertyValue("--col-name")).toBe("minmax(180px, 1fr)");
    expect(loadColumnWidths("left").name).toBeNull();
  });

  it("keeps panes independent and keeps the header aligned while scrolling", () => {
    const first = renderList({ pane: "left" });
    const leftTable = first.container.querySelector<HTMLElement>(".file-table")!;
    const leftSize = screen.getByRole("separator", { name: "Resize Size column" });
    fireEvent.pointerDown(leftSize, { button: 0, clientX: 100, pointerId: 8 });
    fireEvent.pointerMove(leftSize, { clientX: 150, pointerId: 8 });
    fireEvent.pointerUp(leftSize, { pointerId: 8 });
    expect(leftTable.style.getPropertyValue("--col-size")).toBe("142px");
    first.unmount();
    cleanup();

    const second = renderList({ pane: "right" });
    const rightTable = second.container.querySelector<HTMLElement>(".file-table")!;
    expect(rightTable.style.getPropertyValue("--col-size")).toBe("92px");
    expect(leftTable.style.getPropertyValue("--col-size")).toBe("142px");

    const header = second.container.querySelector<HTMLElement>(".file-table-header")!;
    const viewport = second.container.querySelector<HTMLElement>(".file-list-viewport")!;
    viewport.scrollLeft = 120;
    fireEvent.scroll(viewport);
    expect(header.style.transform).toBe("translateX(-120px)");
    expect(rightTable.style.getPropertyValue("--table-min-width")).toBe("520px");
  });

  it("renders the adjustable header for an empty directory", () => {
    renderList({ emptyState: <span>Empty folder</span>, items: [] });
    expect(screen.getByText("Empty folder")).toBeInTheDocument();
    expect(screen.getByRole("separator", { name: "Resize Name column" })).toBeInTheDocument();
    expect(screen.getAllByRole("row")).toHaveLength(1);
    expect(screen.getByRole("grid")).toHaveAttribute("aria-rowcount", "1");
  });

  it("keeps the 28px density token synchronized with the virtual row height", () => {
    expect(tokens).toContain(`--file-row-height: ${FILE_ROW_HEIGHT}px;`);
    expect(tokens).toContain("--font-size-file-name: 14px;");
    expect(tokens).toContain("--font-size-sidebar: 13px;");
  });
});
