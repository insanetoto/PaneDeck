import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { I18nProvider } from "../i18n/I18nProvider";
import type { OperationJob } from "../browser/operationApi";
import { ConflictDialog } from "./ConflictDialog";

afterEach(cleanup);

function conflictJob(kind: "fileReplacement" | "directoryMerge" | "typeMismatch"): OperationJob {
  return {
    jobId: 9,
    kind: "copy",
    state: "awaitingDecision",
    itemCount: 1,
    completedItems: 0,
    failedItems: 0,
    completedUnits: null,
    totalUnits: null,
    bytesPerSecond: null,
    elapsedMillis: 0,
    itemOutcomes: [],
    canCancel: true,
    conflict: {
      kind,
      replaceAllowed: kind !== "typeMismatch",
      source: {
        name: "incoming.txt",
        parentContext: "/source",
        kind: "file",
        byteLen: 12,
        modifiedMillis: 1_700_000_000_000,
      },
      target: {
        name: "incoming.txt",
        parentContext: "/destination",
        kind: kind === "typeMismatch" ? "directory" : "file",
        byteLen: 8,
        modifiedMillis: 1_700_000_001_000,
      },
    },
  };
}

it("focuses the non-destructive skip action and supports apply-to-remaining", () => {
  const resolve = vi.fn();
  render(
    <I18nProvider>
      <ConflictDialog job={conflictJob("fileReplacement")} onClose={vi.fn()} onResolve={resolve} />
    </I18nProvider>,
  );
  expect(screen.getByRole("button", { name: "Skip" })).toHaveFocus();
  fireEvent.click(screen.getByRole("checkbox"));
  fireEvent.click(screen.getByRole("button", { name: "Keep both" }));
  expect(resolve).toHaveBeenCalledWith("keepBoth", true);
});

it("uses merge wording for folders and disables invalid type replacement", () => {
  const { rerender } = render(
    <I18nProvider>
      <ConflictDialog job={conflictJob("directoryMerge")} onClose={vi.fn()} onResolve={vi.fn()} />
    </I18nProvider>,
  );
  expect(screen.getByRole("button", { name: "Merge folders" })).toBeEnabled();
  rerender(
    <I18nProvider>
      <ConflictDialog job={conflictJob("typeMismatch")} onClose={vi.fn()} onResolve={vi.fn()} />
    </I18nProvider>,
  );
  expect(screen.getByRole("button", { name: "Replace existing file" })).toBeDisabled();
});
