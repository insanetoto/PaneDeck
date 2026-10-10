import { useEffect, useState, useSyncExternalStore } from "react";
import type { OperationJob, LogStatus } from "../browser/operationApi";
import { useI18n } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/messages";
import { FeedbackTracker } from "./feedbackTracker";
import { percent, formatBytes } from "./operationModel";
export function BottomFeedback({
  jobs,
  logStatus,
  connectionFailed,
  notice,
  onDismissNotice,
  onCancel,
  onReviewConflict,
  onLog,
  onDiagnostics,
}: {
  jobs: OperationJob[];
  logStatus: LogStatus[];
  connectionFailed: boolean;
  notice: string | null;
  onDismissNotice: () => void;
  onCancel: (id: number) => void;
  onReviewConflict: (job: OperationJob) => void;
  onLog: (id?: string) => void;
  onDiagnostics: () => void;
}) {
  const { t } = useI18n();
  const [tracker] = useState(() => new FeedbackTracker());
  const groups = useSyncExternalStore(tracker.subscribe, tracker.snapshot);
  const [acknowledgedFailures, setAcknowledgedFailures] = useState(0);
  const failedWrites = logStatus.reduce((count, status) => count + status.failedWrites, 0);
  useEffect(() => {
    tracker.update(jobs);
  }, [jobs, tracker]);
  useEffect(() => () => tracker.dispose(), [tracker]);
  const storageFailure = failedWrites > acknowledgedFailures;
  if (!groups.length && !notice && !storageFailure && !connectionFailed) return null;
  return (
    <section className="bottom-feedback" aria-label={t("feedback.label")}>
      {(notice || storageFailure || connectionFailed) && (
        <div className="feedback-notice" role="status">
          <span>
            {storageFailure
              ? t("feedback.logFailed")
              : connectionFailed
                ? t("feedback.connectionFailed")
                : notice}
          </span>
          <button onClick={() => onLog()}>{t("feedback.log")}</button>
          <button onClick={onDiagnostics}>{t("diagnostic.button")}</button>
          {!connectionFailed && (
            <button
              aria-label={t("feedback.dismiss")}
              onClick={() => {
                setAcknowledgedFailures(failedWrites);
                onDismissNotice();
              }}
            >
              ×
            </button>
          )}
        </div>
      )}
      <div className="feedback-cards">
        {groups.map((group) => {
          const job = group.current,
            phase = job.phase ?? (job.state === "running" ? "copying" : "preparing"),
            value = percent(job);
          const outcomes = group.outcomes,
            completed = outcomes.filter((o) => o === "succeeded" || o === "skipped").length;
          return (
            <article className="feedback-card" data-state={group.state} key={group.id}>
              <div className="feedback-heading">
                <strong>{t(`task.kind.${job.kind}` as MessageKey)}</strong>
                <span role="status">
                  {group.active && group.state === "running"
                    ? t(`feedback.phase.${phase}` as MessageKey)
                    : t(`task.state.${group.state}` as MessageKey)}
                </span>
                {!group.active && (
                  <button
                    aria-label={t("feedback.dismiss")}
                    onClick={() => tracker.dismiss(group.id)}
                  >
                    ×
                  </button>
                )}
              </div>
              {group.active &&
                (phase === "copying" && value !== null ? (
                  <progress max={100} value={value} aria-label={t("feedback.progress")} />
                ) : (
                  <progress aria-label={t("feedback.progress")} />
                ))}
              <div className="feedback-metrics">
                <span>
                  {t("task.items")
                    .replace(
                      "{completed}",
                      String(
                        group.active
                          ? group.children.reduce((n, child) => n + child.completedItems, 0)
                          : completed,
                      ),
                    )
                    .replace(
                      "{total}",
                      String(
                        group.active
                          ? (job.operationItemCount ??
                              group.children.reduce((n, child) => n + child.itemCount, 0))
                          : outcomes.length,
                      ),
                    )}
                </span>
                {job.completedUnits !== null && (
                  <span>
                    {formatBytes(job.completedUnits)}
                    {job.totalUnits != null && job.totalUnits > 0
                      ? ` / ${formatBytes(job.totalUnits)}`
                      : ""}
                  </span>
                )}
                {phase === "copying" && job.bytesPerSecond !== null && (
                  <span>{formatBytes(job.bytesPerSecond)}/s</span>
                )}
              </div>
              {outcomes.some((o) => o !== "succeeded") && (
                <details>
                  <summary>{t("task.details")}</summary>
                  <ul>
                    {outcomes.map((outcome, index) => (
                      <li key={index}>
                        {t("task.item")
                          .replace("{index}", String(index + 1))
                          .replace("{outcome}", t(`task.outcome.${outcome}` as MessageKey))}
                      </li>
                    ))}
                  </ul>
                </details>
              )}
              {group.children.some((child) => child.itemErrors?.length) && (
                <details>
                  <summary>{t("feedback.errors")}</summary>
                  {group.children
                    .flatMap((child) => child.itemErrors ?? [])
                    .map((error, index) => (
                      <code key={index}>
                        {error.domain}: {error.code}{" "}
                      </code>
                    ))}
                </details>
              )}
              <div className="feedback-actions">
                {group.active && (
                  <button
                    disabled={!job.canCancel}
                    title={!job.canCancel ? t("feedback.cancelUnavailable") : undefined}
                    onClick={() => onCancel(job.jobId)}
                  >
                    {t("task.cancel")}
                  </button>
                )}
                {job.conflict && (
                  <button onClick={() => onReviewConflict(job)}>{t("conflict.review")}</button>
                )}
                <button onClick={() => onLog(job.operationId)}>{t("feedback.log")}</button>
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}
