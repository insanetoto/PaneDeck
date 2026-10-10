import { useEffect, useRef } from "react";
import type { OperationLogSnapshot } from "../browser/operationApi";
import { useI18n } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/messages";
export function OperationLogDialog({
  data,
  operationId,
  onClose,
  onDiagnostics,
}: {
  data: OperationLogSnapshot;
  operationId?: string;
  onClose: () => void;
  onDiagnostics: () => void;
}) {
  const { t } = useI18n();
  const closeButton = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    closeButton.current?.focus();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, []);
  const events = data.events
    .filter((event) => !operationId || event.operationId === operationId)
    .slice()
    .reverse();
  return (
    <div
      className="paste-dialog-backdrop"
      role="dialog"
      aria-modal="true"
      aria-labelledby="operation-log-title"
    >
      <section className="operation-log-dialog">
        <header>
          <h2 id="operation-log-title">{t("feedback.log")}</h2>
          <button ref={closeButton} aria-label={t("feedback.dismiss")} onClick={onClose}>
            ×
          </button>
        </header>
        {data.status.failedWrites + data.recoveryStatus.failedWrites > 0 && (
          <p role="alert">{t("feedback.logFailed")}</p>
        )}
        <div className="operation-log-events">
          {events.length ? (
            events.map((event, index) => (
              <article key={index}>
                <strong>
                  {t(`task.kind.${event.kind}` as MessageKey)} ·{" "}
                  {[
                    "requested",
                    "queued",
                    "conflict",
                    "decision",
                    "jobFinished",
                    "finished",
                    "rejected",
                  ].includes(event.event)
                    ? t(`feedback.event.${event.event}` as MessageKey)
                    : event.event}
                  {event.state ? ` · ${t(`task.state.${event.state}` as MessageKey)}` : ""}
                </strong>
                <time>{new Date(event.timestampMillis).toLocaleString()}</time>
                <code>
                  {event.operationId}
                  {event.jobId !== null ? ` / ${event.jobId}` : ""}
                </code>
                {event.outcomes.map((outcome, index) => (
                  <div key={index}>
                    {t("task.item")
                      .replace("{index}", String(index + 1))
                      .replace("{outcome}", t(`task.outcome.${outcome}` as MessageKey))}
                  </div>
                ))}
                {event.errors.map((error, index) => (
                  <code key={index}>
                    {error.domain}: {error.code}
                  </code>
                ))}
              </article>
            ))
          ) : (
            <p>{t("feedback.logEmpty")}</p>
          )}
        </div>
        <button
          onClick={() => {
            onClose();
            onDiagnostics();
          }}
        >
          {t("diagnostic.button")}
        </button>
      </section>
    </div>
  );
}
