import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { I18nProvider, LANGUAGE_STORAGE_KEY } from "../i18n/I18nProvider";
import type { OperationJob } from "../browser/operationApi";
import { BottomFeedback } from "./BottomFeedback";
import { FeedbackTracker } from "./feedbackTracker";
import { operationGroups, percent } from "./operationModel";
export const job = (overrides: Partial<OperationJob> = {}): OperationJob => ({
  jobId: 1,
  kind: "copy",
  state: "running",
  itemCount: 2,
  completedItems: 0,
  failedItems: 0,
  completedUnits: null,
  totalUnits: null,
  bytesPerSecond: null,
  elapsedMillis: 0,
  itemOutcomes: [],
  canCancel: true,
  phase: "copying",
  ...overrides,
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
describe("bottom feedback", () => {
  it("does not flash short tasks and closes a shown success after two seconds", () => {
    vi.useFakeTimers();
    const tracker = new FeedbackTracker();
    tracker.update([job()]);
    vi.advanceTimersByTime(200);
    tracker.update([job({ state: "completed", itemOutcomes: ["succeeded"] })]);
    vi.advanceTimersByTime(500);
    expect(tracker.snapshot()).toEqual([]);
    tracker.update([job({ jobId: 2 })]);
    vi.advanceTimersByTime(499);
    expect(tracker.snapshot()).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(tracker.snapshot()).toHaveLength(1);
    tracker.update([job({ jobId: 2, state: "completed", itemOutcomes: ["succeeded"] })]);
    vi.advanceTimersByTime(1999);
    expect(tracker.snapshot()).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(tracker.snapshot()).toEqual([]);
    tracker.dispose();
    expect(vi.getTimerCount()).toBe(0);
  });
  it("retains short failures and source-not-removed results until dismissed", () => {
    vi.useFakeTimers();
    const tracker = new FeedbackTracker();
    tracker.update([
      job({
        kind: "rename",
        state: "partiallyFailed",
        itemOutcomes: ["copiedButSourceNotRemoved"],
      }),
    ]);
    vi.advanceTimersByTime(10_000);
    expect(tracker.snapshot()).toHaveLength(1);
    tracker.dismiss("job-1");
    expect(tracker.snapshot()).toEqual([]);
    tracker.dispose();
  });
  it("does not complete a split logical operation before its final callback", () => {
    const jobs = [
      job({ operationId: "logical", operationState: null, state: "completed" }),
      job({ jobId: 2, operationId: "logical", state: "queued" }),
    ];
    expect(operationGroups(jobs)).toHaveLength(1);
    expect(operationGroups(jobs)[0].active).toBe(true);
    expect(
      operationGroups([
        job({ operationId: "logical", state: "completed", operationState: null }),
      ])[0].active,
    ).toBe(true);
    expect(percent(job({ totalUnits: 0, completedUnits: 0 }))).toBeNull();
  });
  it("shows indeterminate verification and explains unavailable cancellation without taking focus", () => {
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "en");
    const onCancel = vi.fn();
    const input = document.createElement("input");
    document.body.append(input);
    input.focus();
    const view = render(
      <I18nProvider>
        <BottomFeedback
          jobs={[
            job({
              elapsedMillis: 600,
              phase: "verifying",
              totalUnits: 100,
              completedUnits: 100,
              canCancel: false,
            }),
          ]}
          logStatus={[]}
          connectionFailed={false}
          notice={null}
          onDismissNotice={vi.fn()}
          onCancel={onCancel}
          onReviewConflict={vi.fn()}
          onLog={vi.fn()}
          onDiagnostics={vi.fn()}
        />
      </I18nProvider>,
    );
    expect(screen.getByRole("progressbar")).not.toHaveAttribute("value");
    expect(screen.getByText("Verifying")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel task" })).toBeDisabled();
    expect(input).toHaveFocus();
    view.unmount();
    input.remove();
  });
  it("does not announce finished while a split operation awaits its final callback", () => {
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "en");
    render(
      <I18nProvider>
        <BottomFeedback
          jobs={[
            job({
              operationId: "logical",
              operationState: null,
              phase: "finished",
              state: "completed",
              elapsedMillis: 600,
              canCancel: false,
            }),
          ]}
          logStatus={[]}
          connectionFailed={false}
          notice={null}
          onDismissNotice={vi.fn()}
          onCancel={vi.fn()}
          onReviewConflict={vi.fn()}
          onLog={vi.fn()}
          onDiagnostics={vi.fn()}
        />
      </I18nProvider>,
    );
    expect(screen.queryByText("Finished")).not.toBeInTheDocument();
    expect(screen.getByText("Preparing")).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).not.toHaveAttribute("value");
  });

  it("keeps log failure visible with trace and diagnostic actions", () => {
    window.localStorage.setItem(LANGUAGE_STORAGE_KEY, "en");
    const log = vi.fn(),
      diagnostic = vi.fn();
    render(
      <I18nProvider>
        <BottomFeedback
          jobs={[]}
          logStatus={[{ persistenceEnabled: true, failedWrites: 1, lastError: "PermissionDenied" }]}
          connectionFailed={false}
          notice={null}
          onDismissNotice={vi.fn()}
          onCancel={vi.fn()}
          onReviewConflict={vi.fn()}
          onLog={log}
          onDiagnostics={diagnostic}
        />
      </I18nProvider>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Operation log" }));
    fireEvent.click(screen.getByRole("button", { name: "Diagnostics" }));
    expect(log).toHaveBeenCalled();
    expect(diagnostic).toHaveBeenCalled();
  });
});
