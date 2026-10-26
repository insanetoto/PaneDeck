import { useEffect, useRef, useState } from "react";

import { useI18n } from "../i18n/I18nProvider";
import type { MessageKey } from "../i18n/messages";
import type { UninstallClient, UninstallPreview, UninstallResult } from "../browser/uninstallApi";

export interface UninstallDialogProps {
  open: boolean;
  client: UninstallClient;
  onClose: () => void;
}

function formatBytes(value: string | null): string {
  if (value === null) return "—";
  const bytes = Number(value);
  if (!Number.isFinite(bytes)) return "—";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 ** 2) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 ** 3) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
}

export function UninstallDialog({ open, client, onClose }: UninstallDialogProps) {
  const { t } = useI18n();
  const [preview, setPreview] = useState<UninstallPreview | null>(null);
  const [removeData, setRemoveData] = useState(true);
  const [trashApp, setTrashApp] = useState(true);
  const [loading, setLoading] = useState(true);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<UninstallResult | null>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void client
      .preview()
      .then((value) => {
        if (cancelled) return;
        setPreview(value);
        setTrashApp(value.appBundleExists);
        setRemoveData(true);
      })
      .catch((reason) => {
        if (!cancelled) setError(String(reason));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [client, open]);

  useEffect(() => {
    if (open) closeRef.current?.focus();
  }, [open]);

  if (!open) return null;

  const confirm = async () => {
    if (!preview || (!removeData && !trashApp) || working) return;
    setWorking(true);
    setError(null);
    try {
      const value = await client.uninstall({ removeData, trashApp });
      setResult(value);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setWorking(false);
    }
  };

  return (
    <div
      className="paste-dialog-backdrop"
      role="dialog"
      aria-modal="true"
      aria-labelledby="uninstall-title"
    >
      <section className="paste-dialog uninstall-dialog">
        <header>
          <h2 id="uninstall-title">{t("uninstall.title")}</h2>
          <button
            aria-label={t("feedback.dismiss")}
            disabled={working || Boolean(result)}
            onClick={onClose}
            ref={closeRef}
            type="button"
          >
            ×
          </button>
        </header>
        {loading ? <p>{t("uninstall.loading")}</p> : null}
        {error ? (
          <p className="uninstall-error" role="alert">
            {error}
          </p>
        ) : null}
        {preview && !result ? (
          <>
            <p>{t("uninstall.description")}</p>
            <h3>{t("uninstall.dataHeading")}</h3>
            <ul className="uninstall-paths">
              {preview.paths.map((path) => (
                <li key={path.kind} data-missing={!path.exists || undefined}>
                  <span>{t(`uninstall.kind.${path.kind}` as MessageKey)}</span>
                  <code>{path.path}</code>
                  <small>{path.exists ? formatBytes(path.byteLen) : t("uninstall.missing")}</small>
                </li>
              ))}
            </ul>
            <h3>{t("uninstall.appHeading")}</h3>
            <p className="uninstall-app-path">{preview.appBundle ?? t("uninstall.appMissing")}</p>
            <label className="uninstall-option">
              <input
                checked={removeData}
                onChange={(event) => setRemoveData(event.target.checked)}
                type="checkbox"
              />
              {t("uninstall.removeData")}
            </label>
            <label className="uninstall-option">
              <input
                checked={trashApp}
                disabled={!preview.appBundleExists}
                onChange={(event) => setTrashApp(event.target.checked)}
                type="checkbox"
              />
              {t("uninstall.trashApp")}
            </label>
            <p className="uninstall-note">{t("uninstall.note")}</p>
            <footer className="paste-dialog-actions">
              <button disabled={working} onClick={onClose} type="button">
                {t("uninstall.cancel")}
              </button>
              <button
                className="danger"
                disabled={working || (!removeData && !trashApp)}
                onClick={() => void confirm()}
                type="button"
              >
                {working ? t("uninstall.working") : t("uninstall.confirm")}
              </button>
            </footer>
          </>
        ) : null}
        {result ? (
          <>
            <p role="status">{t("uninstall.done")}</p>
            {result.failed.length > 0 ? (
              <div className="uninstall-error" role="alert">
                <p>{t("uninstall.partial")}</p>
                <ul>
                  {result.failed.map((failure) => (
                    <li key={failure}>{failure}</li>
                  ))}
                </ul>
              </div>
            ) : null}
            <footer className="paste-dialog-actions">
              <button className="primary" onClick={() => void client.quit()} type="button">
                {t("uninstall.quit")}
              </button>
            </footer>
          </>
        ) : null}
      </section>
    </div>
  );
}
