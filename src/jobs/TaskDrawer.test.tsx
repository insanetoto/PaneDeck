import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { OperationJob } from "../browser/operationApi";
import { I18nProvider, LANGUAGE_STORAGE_KEY } from "../i18n/I18nProvider";
import { TaskDrawer } from "./TaskDrawer";

function job(overrides: Partial<OperationJob>): OperationJob {
  return {
    jobId: 1,
    kind: "copy",
    state: "running",
    itemCount: 2,
    completedItems: 0,
    failedItems: 0,
    completedUnits: null,
    totalUnits: null,
    bytesPerSecond: null,
    elapsedMillis: 2_000,
    itemOutcomes: [],
    canCancel: true,
    ...overrides,
  };
}

describe("task drawer", () => {
  afterEach(cleanup);

  it("does not invent a percentage and distinguishes cancellation requested", () => {
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "en");
    const cancel = vi.fn();
    render(
      <I18nProvider>
        <TaskDrawer
          jobs={[
            job({ jobId: 1 }),
            job({ jobId: 2, state: "cancellationRequested", canCancel: false }),
          ]}
          onCancel={cancel}
          onClose={vi.fn()}
        />
      </I18nProvider>,
    );
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(screen.getByText("Cancellation requested")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel task" }));
    expect(cancel).toHaveBeenCalledWith(1);
  });

  it("keeps partial failure details visible", () => {
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "en");
    render(
      <I18nProvider>
        <TaskDrawer
          jobs={[
            job({
              state: "partiallyFailed",
              completedItems: 1,
              failedItems: 1,
              itemOutcomes: ["succeeded", "failed"],
              canCancel: false,
            }),
          ]}
          onCancel={vi.fn()}
          onClose={vi.fn()}
        />
      </I18nProvider>,
    );
    fireEvent.click(screen.getByText("Item results"));
    expect(screen.getByText("Item 2: failed")).toBeInTheDocument();
  });
});
