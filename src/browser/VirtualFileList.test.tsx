import { cleanup, fireEvent, render, screen } from "@testing-library/react";
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
