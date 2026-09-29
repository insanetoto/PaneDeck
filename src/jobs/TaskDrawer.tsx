import type { OperationJob, OperationJobState } from "../browser/operationApi";
import { useI18n } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/messages";

const activeStates = new Set<OperationJobState>([
  "queued",
  "validating",
  "running",
  "awaitingDecision",
  "cancellationRequested",
]);

export function isActiveJob(job: OperationJob) {
  return activeStates.has(job.state);
}

function formatDuration(milliseconds: number) {
  const seconds = Math.floor(milliseconds / 1000);
  if (seconds < 60) return `${seconds}s`;
  return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
}

function formatSpeed(bytesPerSecond: number | null) {
  if (bytesPerSecond === null) return null;
  if (bytesPerSecond < 1024) return `${bytesPerSecond} B/s`;
  if (bytesPerSecond < 1024 ** 2) return `${(bytesPerSecond / 1024).toFixed(1)} KB/s`;
  return `${(bytesPerSecond / 1024 ** 2).toFixed(1)} MB/s`;
}

function stateKey(state: OperationJobState): MessageKey {
  return `task.state.${state}` as MessageKey;
}

interface TaskDrawerProps {
  jobs: OperationJob[];
  onCancel: (jobId: number) => void;
  onClose: () => void;
  onReviewConflict?: (job: OperationJob) => void;
}

export function TaskDrawer({ jobs, onCancel, onClose, onReviewConflict }: TaskDrawerProps) {
  const { t } = useI18n();
  const groups = [
    ["task.active", jobs.filter(isActiveJob)],
    ["task.completed", jobs.filter((job) => job.state === "completed")],
    [
      "task.failed",
      jobs.filter((job) => ["partiallyFailed", "failed", "cancelled"].includes(job.state)),
    ],
  ] as const;

  return (
    <aside aria-label={t("task.drawer")} className="task-drawer">
      <header>
        <strong>{t("task.drawer")}</strong>
        <button aria-label={t("task.close")} onClick={onClose} type="button">
          ×
        </button>
      </header>
      <div aria-live="polite" className="task-groups">
        {groups.map(([label, items]) => (
          <section key={label}>
            <h2>{t(label)}</h2>
            {items.length ? (
              items.map((job) => {
                const determinate = job.totalUnits !== null && job.completedUnits !== null;
                const progress = determinate
                  ? Math.min(100, Math.round((job.completedUnits! / job.totalUnits!) * 100))
                  : null;
                const speed = formatSpeed(job.bytesPerSecond);
                return (
                  <article className="task-card" data-state={job.state} key={job.jobId}>
                    <div className="task-card-title">
                      <strong>{t(`task.kind.${job.kind}` as MessageKey)}</strong>
                      <span>{t(stateKey(job.state))}</span>
                    </div>
                    {progress === null ? (
                      isActiveJob(job) ? (
                        <div className="task-indeterminate" aria-hidden="true" />
                      ) : null
                    ) : (
                      <progress max="100" value={progress}>
                        {progress}%
                      </progress>
                    )}
                    <div className="task-metrics">
                      <span>{formatDuration(job.elapsedMillis)}</span>
                      {speed ? <span>{speed}</span> : null}
                      <span>
                        {t("task.items")
                          .replace("{completed}", String(job.completedItems))
                          .replace("{total}", String(job.itemCount))}
                      </span>
                    </div>
                    {job.itemOutcomes.some((outcome) => outcome !== "succeeded") ? (
                      <details>
                        <summary>{t("task.details")}</summary>
                        <ul>
                          {job.itemOutcomes.map((outcome, index) => (
                            <li key={`${job.jobId}:${index}`}>
                              {t("task.item")
                                .replace("{index}", String(index + 1))
                                .replace("{outcome}", t(`task.outcome.${outcome}` as MessageKey))}
                            </li>
                          ))}
                        </ul>
                      </details>
                    ) : null}
                    {job.canCancel ? (
                      <button onClick={() => onCancel(job.jobId)} type="button">
                        {t("task.cancel")}
                      </button>
                    ) : null}
                    {job.conflict && onReviewConflict ? (
                      <button onClick={() => onReviewConflict(job)} type="button">
                        {t("conflict.review")}
                      </button>
                    ) : null}
                  </article>
                );
              })
            ) : (
              <p className="task-empty">{t("task.none")}</p>
            )}
          </section>
        ))}
      </div>
    </aside>
  );
}
