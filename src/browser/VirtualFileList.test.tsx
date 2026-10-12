import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n/I18nProvider";
import type { EntrySnapshot } from "../ipc/pathReferences";
import type { SortDirection, SortField } from "./directoryApi";
import { VirtualFileList } from "./VirtualFileList";

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

function Harness({ items }: { items: EntrySnapshot[] }) {
  const [field, setField] = useState<SortField>("name");
  const [direction, setDirection] = useState<SortDirection>("ascending");
  return (
    <I18nProvider>
      <VirtualFileList
        entries={items}
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
    expect(screen.getAllByRole("row").length).toBeLessThan(40);
    const viewport = container.querySelector(".file-list-viewport")!;
    fireEvent.scroll(viewport, { target: { scrollTop: 320_000 } });
    expect(screen.getAllByRole("row").length).toBeGreaterThan(1);
    expect(screen.getAllByRole("row").length).toBeLessThan(40);
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
