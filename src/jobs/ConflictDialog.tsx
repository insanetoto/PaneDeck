import { useEffect, useRef, useState } from "react";
import type { ConflictDecision, ConflictEntry, OperationJob } from "../browser/operationApi";
import { useI18n } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/messages";

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 ** 2).toFixed(1)} MB`;
}

function EntryDetails({ entry, label }: { entry: ConflictEntry; label: string }) {
  const { t } = useI18n();
  return (
    <section className="conflict-entry">
      <h3>{label}</h3>
      <strong>{entry.name}</strong>
      <code>{entry.parentContext}</code>
      <dl>
        <div>
          <dt>{t("conflict.type")}</dt>
          <dd>{t(`kind.${entry.kind}` as MessageKey)}</dd>
        </div>
        <div>
          <dt>{t("conflict.size")}</dt>
          <dd>{formatBytes(entry.byteLen)}</dd>
        </div>
        <div>
          <dt>{t("conflict.modified")}</dt>
          <dd>
            {entry.modifiedMillis === null ? "—" : new Date(entry.modifiedMillis).toLocaleString()}
          </dd>
        </div>
      </dl>
    </section>
  );
}

interface ConflictDialogProps {
  job: OperationJob;
  onClose: () => void;
  onResolve: (decision: ConflictDecision, applyToRemaining: boolean) => void;
}

export function ConflictDialog({ job, onClose, onResolve }: ConflictDialogProps) {
  const { t } = useI18n();
  const [applyToRemaining, setApplyToRemaining] = useState(false);
  const safeDefault = useRef<HTMLButtonElement>(null);
  const conflict = job.conflict;
  useEffect(() => safeDefault.current?.focus(), []);
  useEffect(() => {
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);
  if (!conflict) return null;
  const replaceLabel =
    conflict.kind === "directoryMerge" ? t("conflict.merge") : t("conflict.replace");
  const resolve = (decision: ConflictDecision) => onResolve(decision, applyToRemaining);
  return (
    <div aria-modal="true" className="paste-dialog-backdrop" role="dialog">
      <section aria-labelledby="conflict-title" className="conflict-dialog">
        <header>
          <div>
            <h2 id="conflict-title">{t("conflict.title")}</h2>
            <p>{t(`conflict.kind.${conflict.kind}` as MessageKey)}</p>
          </div>
          <button aria-label={t("clipboard.cancel")} onClick={onClose} type="button">
            ×
          </button>
        </header>
        <div className="conflict-comparison">
          <EntryDetails entry={conflict.source} label={t("conflict.source")} />
          <EntryDetails entry={conflict.target} label={t("conflict.target")} />
        </div>
        <label className="conflict-remaining">
          <input
            checked={applyToRemaining}
            onChange={(event) => setApplyToRemaining(event.target.checked)}
            type="checkbox"
          />
          {t("conflict.applyRemaining")}
        </label>
        <div className="conflict-actions">
          <button onClick={() => resolve("skip")} ref={safeDefault} type="button">
            {t("conflict.skip")}
          </button>
          <button onClick={() => resolve("keepBoth")} type="button">
            {t("conflict.keepBoth")}
          </button>
          <button
            className="danger-action"
            disabled={!conflict.replaceAllowed}
            onClick={() => resolve("replace")}
            type="button"
          >
            {replaceLabel}
          </button>
          <button onClick={() => resolve("cancel")} type="button">
            {t("conflict.cancelTask")}
          </button>
        </div>
      </section>
    </div>
  );
}
