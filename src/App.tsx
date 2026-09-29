import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
  type RefObject,
} from "react";
import "./App.css";
import { type Language, useI18n } from "./i18n/I18nProvider";
import type { MessageKey } from "./i18n/messages";
import {
  browseErrorCode,
  tauriDirectoryClient,
  type BrowseErrorCode,
  type BrowseOptions,
  type DirectoryClient,
  type DirectoryListing,
  type PaneId,
  type SortDirection,
  type SortField,
} from "./browser/directoryApi";
import { tauriDirectoryMonitor, type DirectoryMonitor } from "./browser/directoryMonitor";
import {
  favoriteForPath,
  loadFavorites,
  saveFavorites,
  type LocalFavorite,
} from "./browser/favorites";
import {
  tauriLocalLocationsClient,
  type LocalLocation,
  type LocalLocationsClient,
} from "./browser/locationsApi";
import { VirtualFileList } from "./browser/VirtualFileList";
import {
  tauriClipboardClient,
  type ClipboardClient,
  type ClipboardState,
  type PasteDraft,
} from "./browser/clipboardApi";
import {
  tauriOperationClient,
  type OperationClient,
  type OperationJob,
} from "./browser/operationApi";
import {
  tauriDiagnosticClient,
  type DiagnosticClient,
  type DiagnosticPreview,
} from "./browser/diagnosticApi";
import { tauriRecoveryClient, type RecoveryClient, type RecoveryItem } from "./browser/recoveryApi";
import type { EntrySnapshot } from "./ipc/pathReferences";
import { ConflictDialog } from "./jobs/ConflictDialog";
import { isActiveJob, TaskDrawer } from "./jobs/TaskDrawer";

const supportedLanguages: Language[] = ["zh-CN", "en"];
const MIN_SPLIT_PERCENT = 28;
const MAX_SPLIT_PERCENT = 72;
const ACTIVE_TASKS_STORAGE_KEY = "panedeck.active-tasks.v1";

function errorMessageKey(code: BrowseErrorCode): MessageKey {
  return `error.${code}`;
}

interface FilePaneProps {
  active: boolean;
  id: PaneId;
  client: DirectoryClient;
  onActivate: (id: PaneId) => void;
  paneRef: RefObject<HTMLElement | null>;
  monitor?: DirectoryMonitor;
  locations?: LocalLocation[];
  favorites?: LocalFavorite[];
  onAddFavorite?: (favorite: LocalFavorite) => void;
  onRemoveFavorite?: (path: string) => void;
  onDirectoryChange?: (listing: DirectoryListing | null) => void;
  onSelectionChange?: (entries: EntrySnapshot[]) => void;
  cutEntryKeys?: ReadonlySet<string>;
  onEntryContextMenu?: (entry: EntrySnapshot, x: number, y: number) => void;
  refreshToken?: number;
  dragSourcePane?: PaneId | null;
  onEntryDragEnd?: () => void;
  onEntryDragStart?: (entry: EntrySnapshot) => void;
  onEntryDrop?: (entry: EntrySnapshot, copy: boolean) => void;
  onPaneDrop?: (copy: boolean) => void;
}

export function FilePane({
  active,
  client,
  id,
  onActivate,
  paneRef,
  monitor,
  locations = [],
  favorites = [],
  onAddFavorite,
  onRemoveFavorite,
  onDirectoryChange,
  onSelectionChange,
  cutEntryKeys,
  onEntryContextMenu,
  refreshToken = 0,
  dragSourcePane = null,
  onEntryDragEnd,
  onEntryDragStart,
  onEntryDrop,
  onPaneDrop,
}: FilePaneProps) {
  const { t } = useI18n();
  const [showHidden, setShowHidden] = useState(false);
  const [sortField, setSortField] = useState<SortField>("name");
  const [sortDirection, setSortDirection] = useState<SortDirection>("ascending");
  const [history, setHistory] = useState<DirectoryListing[]>([]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  const [pathDraft, setPathDraft] = useState("");
  const [loading, setLoading] = useState(false);
  const [browseError, setBrowseError] = useState<BrowseErrorCode | null>(null);
  const [paneDropTarget, setPaneDropTarget] = useState(false);
  const requestGeneration = useRef(0);
  const refreshRef = useRef<() => void>(() => undefined);
  const pathInputRef = useRef<HTMLInputElement>(null);
  const paneName = t(id === "left" ? "pane.left" : "pane.right");
  const current = history[historyIndex] ?? null;

  useEffect(() => {
    onDirectoryChange?.(current);
    onSelectionChange?.([]);
  }, [current, onDirectoryChange, onSelectionChange]);

  const options = (
    hidden = showHidden,
    field = sortField,
    direction = sortDirection,
  ): BrowseOptions => ({
    pane: id,
    showHidden: hidden,
    sortField: field,
    sortDirection: direction,
  });

  const runNavigation = async (
    action: () => Promise<DirectoryListing>,
    commit: (listing: DirectoryListing) => void,
  ) => {
    const generation = ++requestGeneration.current;
    setLoading(true);
    setBrowseError(null);
    try {
      const listing = await action();
      if (generation !== requestGeneration.current) return;
      commit(listing);
      setPathDraft(listing.displayPath);
    } catch (error) {
      if (generation !== requestGeneration.current) return;
      const code = browseErrorCode(error);
      if (code !== "cancelled") setBrowseError(code);
    } finally {
      if (generation === requestGeneration.current) setLoading(false);
    }
  };

  const pushListing = (listing: DirectoryListing) => {
    setHistory((previous) => [...previous.slice(0, historyIndex + 1), listing].slice(-50));
    setHistoryIndex((previous) => Math.min(previous + 1, 49));
  };

  const replaceListing = (listing: DirectoryListing) => {
    setHistory((previous) =>
      previous.map((item, index) => (index === historyIndex ? listing : item)),
    );
  };

  const openPath = (path: string) =>
    runNavigation(() => client.openPath(path, options()), pushListing);

  const openHistoryIndex = (nextIndex: number) => {
    const target = history[nextIndex];
    if (!target) return;
    void runNavigation(
      () => client.reopen(target.directory, options()),
      (listing) => {
        setHistory((previous) =>
          previous.map((item, index) => (index === nextIndex ? listing : item)),
        );
        setHistoryIndex(nextIndex);
      },
    );
  };

  const openParent = () => {
    if (!current?.canGoUp) return;
    void runNavigation(() => client.openParent(current.directory, options()), pushListing);
  };

  const openEntry = (entry: EntrySnapshot) => {
    if (entry.kind !== "directory" && entry.symlinkTarget !== "directory") return;
    void runNavigation(() => client.openChild(entry.reference, options()), pushListing);
  };

  const updateView = (hidden: boolean, field: SortField, direction: SortDirection) => {
    if (!current) return;
    void runNavigation(
      () => client.reopen(current.directory, options(hidden, field, direction)),
      replaceListing,
    );
  };

  const refreshCurrent = () => {
    if (!current || loading) return;
    void runNavigation(() => client.reopen(current.directory, options()), replaceListing);
  };
  useEffect(() => {
    refreshRef.current = refreshCurrent;
  });

  useEffect(() => {
    if (refreshToken > 0) refreshRef.current();
  }, [refreshToken]);

  const watchedDirectory = current?.directory;
  useEffect(() => {
    if (!monitor || !watchedDirectory) return;
    let disposed = false;
    let stop: (() => void) | undefined;
    void monitor
      .start(id, watchedDirectory, () => refreshRef.current())
      .then((cleanup) => {
        if (disposed) cleanup();
        else stop = cleanup;
      })
      .catch((error) => {
        if (!disposed) setBrowseError(browseErrorCode(error));
      });
    return () => {
      disposed = true;
      stop?.();
    };
  }, [id, monitor, watchedDirectory]);

  const breadcrumbs = (() => {
    if (!current?.displayPath.startsWith("/")) return [];
    const parts = current.displayPath.split("/").filter(Boolean);
    return [
      { label: "/", path: "/" },
      ...parts.map((label, index) => ({ label, path: `/${parts.slice(0, index + 1).join("/")}` })),
    ];
  })();

  const submitPath = (event: FormEvent) => {
    event.preventDefault();
    if (pathDraft.trim()) void openPath(pathDraft.trim());
  };
  const currentIsFavorite = current
    ? favorites.some((favorite) => favorite.path === current.displayPath)
    : false;
  const locationName = (location: LocalLocation) =>
    location.kind === "volume" ? location.name : t(`location.${location.kind}` as MessageKey);

  return (
    <section
      aria-label={paneName}
      className="file-pane"
      data-active={active}
      data-drop-target={paneDropTarget}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
          setPaneDropTarget(false);
        }
      }}
      onDragOver={(event) => {
        if (!current || dragSourcePane === id) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = event.altKey ? "copy" : "move";
        setPaneDropTarget(true);
      }}
      onDrop={(event) => {
        if (!current || dragSourcePane === id) return;
        event.preventDefault();
        setPaneDropTarget(false);
        onPaneDrop?.(event.altKey || event.dataTransfer.dropEffect === "copy");
      }}
      onClick={() => onActivate(id)}
      onFocus={() => onActivate(id)}
      onKeyDown={(event) => {
        if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "l") {
          event.preventDefault();
          pathInputRef.current?.focus();
        } else if (event.metaKey && event.key === "ArrowUp") {
          event.preventDefault();
          openParent();
        }
      }}
      ref={paneRef}
      tabIndex={active ? 0 : -1}
    >
      <header className="pane-navigation">
        <div className="pane-identity">
          <span className="pane-side-label">{paneName}</span>
          {active ? <span className="active-pane-badge">{t("pane.active")}</span> : null}
        </div>
        <div aria-label={t("pane.navigationActions")} className="navigation-actions">
          <button
            aria-label={t("pane.back")}
            disabled={historyIndex <= 0 || loading}
            onClick={() => openHistoryIndex(historyIndex - 1)}
            type="button"
          >
            <span aria-hidden="true">‹</span>
          </button>
          <button
            aria-label={t("pane.forward")}
            disabled={historyIndex < 0 || historyIndex >= history.length - 1 || loading}
            onClick={() => openHistoryIndex(historyIndex + 1)}
            type="button"
          >
            <span aria-hidden="true">›</span>
          </button>
          <button
            aria-label={t("pane.up")}
            disabled={!current?.canGoUp || loading}
            onClick={openParent}
            type="button"
          >
            <span aria-hidden="true">↑</span>
          </button>
          <button
            aria-label={t("pane.refresh")}
            disabled={!current || loading}
            onClick={() => refreshRef.current()}
            type="button"
          >
            <span aria-hidden="true">↻</span>
          </button>
        </div>
        <form className="location-field" onSubmit={submitPath}>
          <span aria-hidden="true">⌂</span>
          <input
            aria-label={t("pane.pathInput")}
            onChange={(event) => setPathDraft(event.target.value)}
            placeholder={t("pane.pathPlaceholder")}
            ref={pathInputRef}
            spellCheck="false"
            value={pathDraft}
          />
        </form>
        <button
          aria-label={t("pane.showHidden")}
          aria-pressed={showHidden}
          className="hidden-toggle"
          onClick={(event) => {
            event.stopPropagation();
            onActivate(id);
            const next = !showHidden;
            setShowHidden(next);
            updateView(next, sortField, sortDirection);
          }}
          type="button"
        >
          <span aria-hidden="true">{showHidden ? "◉" : "○"}</span>
          <span>{t("pane.hidden")}</span>
        </button>
        <button
          aria-label={currentIsFavorite ? t("location.removeFavorite") : t("location.addFavorite")}
          aria-pressed={currentIsFavorite}
          className="favorite-toggle"
          disabled={!current}
          onClick={() => {
            if (!current) return;
            if (currentIsFavorite) onRemoveFavorite?.(current.displayPath);
            else onAddFavorite?.(favoriteForPath(current.displayPath));
          }}
          type="button"
        >
          <span aria-hidden="true">{currentIsFavorite ? "★" : "☆"}</span>
        </button>
      </header>

      <nav aria-label={t("pane.breadcrumbs")} className="breadcrumbs">
        {breadcrumbs.map((crumb) => (
          <button key={crumb.path} onClick={() => void openPath(crumb.path)} type="button">
            {crumb.label}
          </button>
        ))}
      </nav>

      <div className="pane-body">
        <aside aria-label={t("location.sidebar")} className="location-sidebar">
          <strong>{t("location.local")}</strong>
          <div className="location-list">
            {locations
              .filter((location) => location.kind !== "volume")
              .map((location) => (
                <button
                  disabled={!location.available || loading}
                  key={location.id}
                  onClick={() => void openPath(location.path)}
                  title={location.path}
                  type="button"
                >
                  <span aria-hidden="true">⌂</span>
                  {locationName(location)}
                </button>
              ))}
          </div>
          {locations.some((location) => location.kind === "volume") ? (
            <>
              <strong>{t("location.volumes")}</strong>
              <div className="location-list">
                {locations
                  .filter((location) => location.kind === "volume")
                  .map((location) => (
                    <button
                      disabled={!location.available || loading}
                      key={location.id}
                      onClick={() => void openPath(location.path)}
                      title={location.path}
                      type="button"
                    >
                      <span aria-hidden="true">◫</span>
                      {locationName(location)}
                    </button>
                  ))}
              </div>
            </>
          ) : null}
          <strong>{t("location.favorites")}</strong>
          <div className="location-list favorite-list">
            {favorites.length ? (
              favorites.map((favorite) => (
                <div className="favorite-row" key={favorite.path}>
                  <button
                    disabled={loading}
                    onClick={() => void openPath(favorite.path)}
                    title={favorite.path}
                    type="button"
                  >
                    <span aria-hidden="true">★</span>
                    {favorite.name}
                  </button>
                  <button
                    aria-label={`${t("location.removeFavorite")}: ${favorite.name}`}
                    onClick={() => onRemoveFavorite?.(favorite.path)}
                    type="button"
                  >
                    ×
                  </button>
                </div>
              ))
            ) : (
              <span className="no-favorites">{t("location.noFavorites")}</span>
            )}
          </div>
        </aside>

        <div className="file-area">
          {browseError ? (
            <div className="navigation-error" role="alert">
              <span>
                <span aria-hidden="true">⚠ </span>
                {t(errorMessageKey(browseError))}
              </span>
              {current ? (
                <button onClick={() => refreshRef.current()} type="button">
                  {t("pane.retry")}
                </button>
              ) : null}
            </div>
          ) : null}
          {loading && !current ? (
            <div className="empty-pane-state" role="status">
              <strong>{t("pane.loading")}</strong>
            </div>
          ) : current && current.entries.length > 0 ? (
            <VirtualFileList
              cutEntryKeys={cutEntryKeys}
              entries={current.entries}
              onOpen={openEntry}
              onEntryContextMenu={onEntryContextMenu}
              onEntryDragEnd={onEntryDragEnd}
              onEntryDragStart={onEntryDragStart}
              onEntryDrop={onEntryDrop}
              onSelectionChange={onSelectionChange}
              onSort={(field) => {
                const direction =
                  field === sortField && sortDirection === "ascending" ? "descending" : "ascending";
                setSortField(field);
                setSortDirection(direction);
                updateView(showHidden, field, direction);
              }}
              sortDirection={sortDirection}
              sortField={sortField}
            />
          ) : !current && browseError ? (
            <div className="empty-pane-state error-state">
              <strong>{t("error.title")}</strong>
            </div>
          ) : (
            <div className="empty-pane-state">
              <span aria-hidden="true" className="empty-folder-icon">
                ▱
              </span>
              <strong>{current ? t("pane.emptyDirectory") : t("pane.emptyTitle")}</strong>
              <span>
                {current ? t("pane.emptyDirectoryDescription") : t("pane.emptyDescription")}
              </span>
            </div>
          )}
        </div>
      </div>

      <footer className="pane-status">
        <span>
          {current
            ? t("pane.itemCountValue").replace("{count}", String(current.entries.length))
            : t("pane.itemCount")}
        </span>
        <span>
          {loading
            ? t("pane.loading")
            : current?.warnings.length
              ? t("pane.warningCount").replace("{count}", String(current.warnings.length))
              : showHidden
                ? t("pane.hiddenVisible")
                : t("pane.hiddenHidden")}
        </span>
      </footer>
    </section>
  );
}

interface AppProps {
  clipboardClient?: ClipboardClient;
  directoryClient?: DirectoryClient;
  locationsClient?: LocalLocationsClient;
  monitor?: DirectoryMonitor;
  operationClient?: OperationClient;
  diagnosticClient?: DiagnosticClient;
  recoveryClient?: RecoveryClient;
}

function App({
  clipboardClient = tauriClipboardClient,
  directoryClient = tauriDirectoryClient,
  locationsClient = tauriLocalLocationsClient,
  monitor = tauriDirectoryMonitor,
  operationClient = tauriOperationClient,
  diagnosticClient = tauriDiagnosticClient,
  recoveryClient = tauriRecoveryClient,
}: AppProps) {
  const { language, setLanguage, t } = useI18n();
  const [activePane, setActivePane] = useState<PaneId>("left");
  const [splitPercent, setSplitPercent] = useState(50);
  const [locations, setLocations] = useState<LocalLocation[]>([]);
  const [favorites, setFavorites] = useState<LocalFavorite[]>(() => loadFavorites());
  const [paneListings, setPaneListings] = useState<Record<PaneId, DirectoryListing | null>>({
    left: null,
    right: null,
  });
  const [paneSelections, setPaneSelections] = useState<Record<PaneId, EntrySnapshot[]>>({
    left: [],
    right: [],
  });
  const [clipboardState, setClipboardState] = useState<ClipboardState | null>(null);
  const [pasteDraft, setPasteDraft] = useState<PasteDraft | null>(null);
  const [clipboardNotice, setClipboardNotice] = useState<string | null>(null);
  const [operationBusy, setOperationBusy] = useState(false);
  const [jobs, setJobs] = useState<OperationJob[]>([]);
  const [taskDrawerOpen, setTaskDrawerOpen] = useState(false);
  const [conflictJob, setConflictJob] = useState<OperationJob | null>(null);
  const [diagnosticPreview, setDiagnosticPreview] = useState<DiagnosticPreview | null>(null);
  const [diagnosticDestination, setDiagnosticDestination] = useState("");
  const [recoveryItems, setRecoveryItems] = useState<RecoveryItem[]>([]);
  const [interruptedTasks, setInterruptedTasks] = useState(
    () => window.localStorage.getItem(ACTIVE_TASKS_STORAGE_KEY) !== null,
  );
  const [dragState, setDragState] = useState<{
    sourcePane: PaneId;
    entries: EntrySnapshot[];
  } | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);
  const [commandDialog, setCommandDialog] = useState<{
    type: "rename" | "createDirectory" | "trash";
    entries: EntrySnapshot[];
  } | null>(null);
  const [commandName, setCommandName] = useState("");
  const [contextMenu, setContextMenu] = useState<{
    entry: EntrySnapshot;
    x: number;
    y: number;
  } | null>(null);
  const workspaceRef = useRef<HTMLDivElement>(null);
  const leftPaneRef = useRef<HTMLElement>(null);
  const rightPaneRef = useRef<HTMLElement>(null);
  const dragging = useRef(false);
  const terminalJobsSeen = useRef<Set<number>>(new Set());

  const onLeftDirectoryChange = useCallback(
    (listing: DirectoryListing | null) =>
      setPaneListings((current) => ({ ...current, left: listing })),
    [],
  );
  const onRightDirectoryChange = useCallback(
    (listing: DirectoryListing | null) =>
      setPaneListings((current) => ({ ...current, right: listing })),
    [],
  );
  const onLeftSelectionChange = useCallback(
    (entries: EntrySnapshot[]) => setPaneSelections((current) => ({ ...current, left: entries })),
    [],
  );
  const onRightSelectionChange = useCallback(
    (entries: EntrySnapshot[]) => setPaneSelections((current) => ({ ...current, right: entries })),
    [],
  );

  useEffect(() => {
    let disposed = false;
    void locationsClient
      .list()
      .then((result) => {
        if (!disposed) setLocations(result.locations);
      })
      .catch(() => {
        if (!disposed) setLocations([]);
      });
    return () => {
      disposed = true;
    };
  }, [locationsClient]);

  useEffect(() => {
    void recoveryClient
      .list()
      .then(setRecoveryItems)
      .catch(() => setRecoveryItems([]));
  }, [recoveryClient]);

  useEffect(() => {
    const closeTopDialog = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (diagnosticPreview) {
        setDiagnosticPreview(null);
      } else if (commandDialog) {
        setCommandDialog(null);
      } else if (pasteDraft) {
        const draftId = pasteDraft.draftId;
        setPasteDraft(null);
        void clipboardClient.cancel(draftId);
      }
    };
    window.addEventListener("keydown", closeTopDialog);
    return () => window.removeEventListener("keydown", closeTopDialog);
  }, [clipboardClient, commandDialog, diagnosticPreview, pasteDraft]);

  useEffect(() => {
    let disposed = false;
    const refreshJobs = async () => {
      try {
        const next = await operationClient.listJobs();
        if (disposed) return;
        setJobs(next);
        const active = next.filter(isActiveJob);
        if (active.length) {
          window.localStorage.setItem(
            ACTIVE_TASKS_STORAGE_KEY,
            JSON.stringify(active.map((job) => job.jobId)),
          );
        } else {
          window.localStorage.removeItem(ACTIVE_TASKS_STORAGE_KEY);
        }
        let terminalChanged = false;
        for (const job of next) {
          if (!isActiveJob(job) && !terminalJobsSeen.current.has(job.jobId)) {
            terminalJobsSeen.current.add(job.jobId);
            terminalChanged = true;
          }
        }
        if (terminalChanged) setRefreshToken((value) => value + 1);
      } catch {
        // Tauri is unavailable in isolated web tests and previews.
      }
    };
    void refreshJobs();
    const timer = window.setInterval(() => void refreshJobs(), 250);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  }, [operationClient]);

  const updateFavorites = (next: LocalFavorite[]) => {
    setFavorites(next);
    saveFavorites(next);
  };

  const addFavorite = (favorite: LocalFavorite) => {
    if (favorites.some((existing) => existing.path === favorite.path)) return;
    updateFavorites([...favorites, favorite]);
  };

  const removeFavorite = (path: string) => {
    updateFavorites(favorites.filter((favorite) => favorite.path !== path));
  };

  const activatePane = (pane: PaneId, moveFocus = false) => {
    setActivePane(pane);
    if (moveFocus) {
      const target = pane === "left" ? leftPaneRef.current : rightPaneRef.current;
      target?.focus();
    }
  };

  const updateSplit = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!dragging.current || !workspaceRef.current) return;
    const bounds = workspaceRef.current.getBoundingClientRect();
    if (bounds.width <= 0) return;
    const requested = ((event.clientX - bounds.left) / bounds.width) * 100;
    setSplitPercent(Math.min(MAX_SPLIT_PERCENT, Math.max(MIN_SPLIT_PERCENT, requested)));
  };

  const workspaceStyle = { "--left-pane-width": `${splitPercent}%` } as CSSProperties;
  const cutEntryKeys = new Set(
    clipboardState?.cutEntries.map((reference) => `${reference.sessionId}:${reference.entryId}`) ??
      [],
  );

  const setClipboard = async (intent: "copy" | "cut", explicit?: EntrySnapshot[]) => {
    const entries = explicit ?? paneSelections[activePane];
    if (!entries.length) return;
    try {
      const next = await clipboardClient.set(
        activePane,
        intent,
        entries.map((entry) => entry.reference),
      );
      setClipboardState(next);
      setClipboardNotice(
        t(intent === "cut" ? "clipboard.cutReady" : "clipboard.copyReady").replace(
          "{count}",
          String(next.itemCount),
        ),
      );
    } catch {
      setClipboardNotice(t("clipboard.error"));
    }
  };

  const finishOperation = (job: OperationJob) => {
    setJobs((current) => [job, ...current.filter((item) => item.jobId !== job.jobId)]);
    setTaskDrawerOpen(true);
    window.localStorage.setItem(ACTIVE_TASKS_STORAGE_KEY, JSON.stringify([job.jobId]));
    setClipboardNotice(t("operation.queued"));
  };

  const runOperation = async (operation: () => Promise<OperationJob>) => {
    if (operationBusy) return;
    setOperationBusy(true);
    setContextMenu(null);
    try {
      finishOperation(await operation());
    } catch {
      setClipboardNotice(t("operation.failed"));
    } finally {
      setOperationBusy(false);
    }
  };

  const cancelJob = (jobId: number) => {
    void operationClient
      .cancel(jobId)
      .then((job) =>
        setJobs((current) => current.map((item) => (item.jobId === jobId ? job : item))),
      )
      .catch(() => setClipboardNotice(t("operation.failed")));
  };

  const resolveConflict = (
    job: OperationJob,
    decision: "skip" | "replace" | "keepBoth" | "cancel",
    applyToRemaining: boolean,
  ) => {
    setConflictJob(null);
    void operationClient
      .resolveConflict(job.jobId, decision, applyToRemaining)
      .then(finishOperation)
      .catch(() => setClipboardNotice(t("operation.failed")));
  };

  const openDiagnostics = () => {
    void diagnosticClient
      .preview()
      .then((preview) => {
        setDiagnosticPreview(preview);
        setDiagnosticDestination("");
      })
      .catch(() => setClipboardNotice(t("diagnostic.error")));
  };

  const exportDiagnostics = () => {
    if (!diagnosticPreview || !diagnosticDestination.trim()) return;
    void diagnosticClient
      .export(diagnosticDestination.trim(), diagnosticPreview.content)
      .then(() => {
        setDiagnosticPreview(null);
        setClipboardNotice(t("diagnostic.saved"));
      })
      .catch(() => setClipboardNotice(t("diagnostic.error")));
  };

  const cleanupRecovery = (item: RecoveryItem) => {
    if (!window.confirm(t("recovery.confirmCleanup"))) return;
    void recoveryClient
      .confirmCleanup(item.recoveryId)
      .then(() =>
        setRecoveryItems((current) =>
          current.filter((candidate) => candidate.recoveryId !== item.recoveryId),
        ),
      )
      .catch(() => setClipboardNotice(t("recovery.cleanupFailed")));
  };

  const transfer = (kind: "copy" | "move", explicit?: EntrySnapshot[]) => {
    const entries = explicit ?? paneSelections[activePane];
    const destinationPane: PaneId = activePane === "left" ? "right" : "left";
    const destination = paneListings[destinationPane]?.directory;
    if (!entries.length || !destination) return;
    void runOperation(() =>
      operationClient.transfer(
        kind,
        entries.map((entry) => entry.reference),
        destination,
      ),
    );
  };

  const dropOnPane = (targetPane: PaneId, copy: boolean) => {
    if (!dragState || dragState.sourcePane === targetPane) return;
    const destination = paneListings[targetPane]?.directory;
    if (!destination) return;
    void runOperation(() =>
      operationClient.transfer(
        copy ? "copy" : "move",
        dragState.entries.map((entry) => entry.reference),
        destination,
      ),
    );
    setDragState(null);
  };

  const dropOnEntry = (target: EntrySnapshot, copy: boolean) => {
    if (!dragState || target.isReadOnly) return;
    const targetKey = `${target.reference.sessionId}:${target.reference.entryId}`;
    if (
      dragState.entries.some(
        (entry) => `${entry.reference.sessionId}:${entry.reference.entryId}` === targetKey,
      )
    ) {
      setClipboardNotice(t("operation.invalidDrop"));
      setDragState(null);
      return;
    }
    void runOperation(() =>
      operationClient.transferToEntry(
        copy ? "copy" : "move",
        dragState.entries.map((entry) => entry.reference),
        target.reference,
      ),
    );
    setDragState(null);
  };

  const openRename = (entries = paneSelections[activePane]) => {
    if (entries.length !== 1) return;
    setCommandName(entries[0].displayName);
    setCommandDialog({ type: "rename", entries });
    setContextMenu(null);
  };

  const openCreateDirectory = () => {
    if (!paneListings[activePane]) return;
    setCommandName("");
    setCommandDialog({ type: "createDirectory", entries: [] });
    setContextMenu(null);
  };

  const openTrash = (entries = paneSelections[activePane]) => {
    if (!entries.length) return;
    setCommandDialog({ type: "trash", entries });
    setContextMenu(null);
  };

  const submitCommand = () => {
    if (!commandDialog) return;
    const dialog = commandDialog;
    setCommandDialog(null);
    if (dialog.type === "rename") {
      if (!commandName.trim()) return;
      void runOperation(() => operationClient.rename(dialog.entries[0].reference, commandName));
    } else if (dialog.type === "createDirectory") {
      const directory = paneListings[activePane]?.directory;
      if (!directory || !commandName.trim()) return;
      void runOperation(() => operationClient.createDirectory(directory, commandName));
    } else {
      void runOperation(() =>
        operationClient.trash(dialog.entries.map((entry) => entry.reference)),
      );
    }
  };

  const preparePaste = async () => {
    const source = clipboardState?.sourcePane;
    const otherPane: PaneId = source === "left" ? "right" : "left";
    const preferred = source && paneListings[otherPane] ? otherPane : activePane;
    const fallback: PaneId = preferred === "left" ? "right" : "left";
    const target = paneListings[preferred] ? preferred : paneListings[fallback] ? fallback : null;
    if (!target) {
      setClipboardNotice(t("clipboard.noTarget"));
      return;
    }
    try {
      setPasteDraft(await clipboardClient.prepare(target, paneListings[target]!.directory));
      setClipboardNotice(null);
    } catch {
      setClipboardNotice(t("clipboard.error"));
    }
  };

  const handleShortcut = (event: ReactKeyboardEvent<HTMLElement>) => {
    const target = event.target as HTMLElement;
    if (target.matches("input, textarea, [contenteditable='true']")) return;
    if (event.key === "F5" || event.key === "F6" || event.key === "F2") {
      event.preventDefault();
      if (event.key === "F5") transfer("copy");
      else if (event.key === "F6") transfer("move");
      else openRename();
      return;
    }
    if (event.metaKey && event.shiftKey && event.key.toLowerCase() === "n") {
      event.preventDefault();
      openCreateDirectory();
      return;
    }
    if (event.metaKey && event.key === "Backspace") {
      event.preventDefault();
      openTrash();
      return;
    }
    if (!event.metaKey || event.altKey || event.ctrlKey) return;
    const key = event.key.toLowerCase();
    if (key === "c" || key === "x") {
      event.preventDefault();
      void setClipboard(key === "x" ? "cut" : "copy");
    } else if (key === "v") {
      event.preventDefault();
      void preparePaste();
    }
  };

  const activeSelection = paneSelections[activePane];
  const otherPane: PaneId = activePane === "left" ? "right" : "left";
  const hasTransferTarget = Boolean(paneListings[otherPane]);
  const selectionDisabledReason = activeSelection.length
    ? undefined
    : t("operation.disabled.selection");
  const transferDisabledReason =
    selectionDisabledReason ??
    (hasTransferTarget ? undefined : t("operation.disabled.destination"));

  return (
    <main
      aria-label={t("app.name")}
      className="app-shell"
      onKeyDown={(event) => {
        handleShortcut(event);
        if (event.defaultPrevented) return;
        if (event.key !== "Tab" || event.altKey || !event.ctrlKey || event.metaKey) return;
        event.preventDefault();
        activatePane(activePane === "left" ? "right" : "left", true);
      }}
    >
      <header className="app-toolbar">
        <div className="compact-brand">
          <span aria-hidden="true" className="brand-mark">
            <span />
            <span />
          </span>
          <div>
            <strong>{t("app.name")}</strong>
            <span>{t("app.tagline")}</span>
          </div>
        </div>

        <div aria-label={t("operation.toolbar")} className="operation-toolbar">
          <button
            disabled={Boolean(transferDisabledReason) || operationBusy}
            onClick={() => transfer("copy")}
            title={transferDisabledReason}
            type="button"
          >
            {t("operation.copy")}
          </button>
          <button
            disabled={Boolean(transferDisabledReason) || operationBusy}
            onClick={() => transfer("move")}
            title={transferDisabledReason}
            type="button"
          >
            {t("operation.move")}
          </button>
          <button
            disabled={Boolean(selectionDisabledReason) || operationBusy}
            onClick={() => void setClipboard("cut")}
            title={selectionDisabledReason}
            type="button"
          >
            {t("operation.cut")}
          </button>
          <button
            disabled={!paneListings[activePane] || operationBusy}
            onClick={() => void preparePaste()}
            title={!paneListings[activePane] ? t("operation.disabled.destination") : undefined}
            type="button"
          >
            {t("operation.paste")}
          </button>
          <button
            disabled={activeSelection.length !== 1 || operationBusy}
            onClick={() => openRename()}
            title={activeSelection.length !== 1 ? t("operation.disabled.single") : undefined}
            type="button"
          >
            {t("operation.rename")}
          </button>
          <button
            disabled={!paneListings[activePane] || operationBusy}
            onClick={openCreateDirectory}
            title={!paneListings[activePane] ? t("operation.disabled.destination") : undefined}
            type="button"
          >
            {t("operation.newFolder")}
          </button>
          <button
            disabled={Boolean(selectionDisabledReason) || operationBusy}
            onClick={() => openTrash()}
            title={selectionDisabledReason}
            type="button"
          >
            {t("operation.trash")}
          </button>
          <button onClick={() => setTaskDrawerOpen((value) => !value)} type="button">
            {t("task.button").replace("{count}", String(jobs.filter(isActiveJob).length))}
          </button>
          <button onClick={openDiagnostics} type="button">
            {t("diagnostic.button")}
          </button>
        </div>

        <div className="toolbar-actions">
          <span className="keyboard-hint">{t("pane.tabHint")}</span>
          <div aria-label={t("language.label")} className="segmented-control">
            {supportedLanguages.map((candidate) => (
              <button
                aria-pressed={language === candidate}
                key={candidate}
                onClick={() => setLanguage(candidate)}
                type="button"
              >
                {t(candidate === "zh-CN" ? "language.zhCNShort" : "language.enShort")}
              </button>
            ))}
          </div>
        </div>
      </header>

      <div className="workspace" ref={workspaceRef} style={workspaceStyle}>
        <FilePane
          active={activePane === "left"}
          client={directoryClient}
          favorites={favorites}
          id="left"
          locations={locations}
          monitor={monitor}
          onAddFavorite={addFavorite}
          onActivate={activatePane}
          onRemoveFavorite={removeFavorite}
          onDirectoryChange={onLeftDirectoryChange}
          onSelectionChange={onLeftSelectionChange}
          cutEntryKeys={cutEntryKeys}
          dragSourcePane={dragState?.sourcePane}
          onEntryContextMenu={(entry, x, y) => {
            activatePane("left");
            setContextMenu({ entry, x, y });
          }}
          onEntryDragEnd={() => setDragState(null)}
          onEntryDragStart={(entry) => {
            activatePane("left");
            const selected = paneSelections.left.some(
              (candidate) =>
                candidate.reference.sessionId === entry.reference.sessionId &&
                candidate.reference.entryId === entry.reference.entryId,
            );
            setDragState({
              sourcePane: "left",
              entries: selected ? paneSelections.left : [entry],
            });
          }}
          onEntryDrop={dropOnEntry}
          onPaneDrop={(copy) => dropOnPane("left", copy)}
          paneRef={leftPaneRef}
          refreshToken={refreshToken}
        />
        <div
          aria-label={t("pane.resize")}
          aria-orientation="vertical"
          aria-valuemax={MAX_SPLIT_PERCENT}
          aria-valuemin={MIN_SPLIT_PERCENT}
          aria-valuenow={Math.round(splitPercent)}
          aria-valuetext={`${Math.round(splitPercent)}%`}
          className="pane-divider"
          onDoubleClick={() => setSplitPercent(50)}
          onPointerCancel={() => {
            dragging.current = false;
          }}
          onPointerDown={(event) => {
            dragging.current = true;
            event.currentTarget.setPointerCapture?.(event.pointerId);
          }}
          onPointerMove={updateSplit}
          onPointerUp={(event) => {
            dragging.current = false;
            event.currentTarget.releasePointerCapture?.(event.pointerId);
          }}
          onKeyDown={(event) => {
            if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
            event.preventDefault();
            const delta = event.key === "ArrowLeft" ? -2 : 2;
            setSplitPercent((value) =>
              Math.min(MAX_SPLIT_PERCENT, Math.max(MIN_SPLIT_PERCENT, value + delta)),
            );
          }}
          role="separator"
          tabIndex={0}
        >
          <span aria-hidden="true" />
        </div>
        <FilePane
          active={activePane === "right"}
          client={directoryClient}
          favorites={favorites}
          id="right"
          locations={locations}
          monitor={monitor}
          onAddFavorite={addFavorite}
          onActivate={activatePane}
          onRemoveFavorite={removeFavorite}
          onDirectoryChange={onRightDirectoryChange}
          onSelectionChange={onRightSelectionChange}
          cutEntryKeys={cutEntryKeys}
          dragSourcePane={dragState?.sourcePane}
          onEntryContextMenu={(entry, x, y) => {
            activatePane("right");
            setContextMenu({ entry, x, y });
          }}
          onEntryDragEnd={() => setDragState(null)}
          onEntryDragStart={(entry) => {
            activatePane("right");
            const selected = paneSelections.right.some(
              (candidate) =>
                candidate.reference.sessionId === entry.reference.sessionId &&
                candidate.reference.entryId === entry.reference.entryId,
            );
            setDragState({
              sourcePane: "right",
              entries: selected ? paneSelections.right : [entry],
            });
          }}
          onEntryDrop={dropOnEntry}
          onPaneDrop={(copy) => dropOnPane("right", copy)}
          paneRef={rightPaneRef}
          refreshToken={refreshToken}
        />
      </div>
      {clipboardNotice ? (
        <div className="clipboard-notice" role="status">
          {clipboardNotice}
        </div>
      ) : null}
      {interruptedTasks ? (
        <div className="restart-warning" role="alert">
          <span>{t("task.restartWarning")}</span>
          <button
            onClick={() => {
              window.localStorage.removeItem(ACTIVE_TASKS_STORAGE_KEY);
              setInterruptedTasks(false);
            }}
            type="button"
          >
            {t("task.dismiss")}
          </button>
        </div>
      ) : null}
      {recoveryItems.length ? (
        <section aria-label={t("recovery.title")} className="recovery-warning" role="alert">
          <div>
            <strong>{t("recovery.title")}</strong>
            <p>{t("recovery.description")}</p>
          </div>
          {recoveryItems.map((item) => (
            <div className="recovery-item" key={item.recoveryId}>
              <span>
                {t("recovery.item")
                  .replace("{kind}", item.kind)
                  .replace("{checkpoint}", item.checkpoint)
                  .replace("{sources}", String(item.sourceItemsPresent))
                  .replace("{residuals}", String(item.residualItems))}
              </span>
              {item.residualItems && item.cleanupAvailable ? (
                <button onClick={() => cleanupRecovery(item)} type="button">
                  {t("recovery.cleanup")}
                </button>
              ) : null}
            </div>
          ))}
        </section>
      ) : null}
      {taskDrawerOpen ? (
        <TaskDrawer
          jobs={jobs}
          onCancel={cancelJob}
          onClose={() => setTaskDrawerOpen(false)}
          onReviewConflict={setConflictJob}
        />
      ) : null}
      {conflictJob ? (
        <ConflictDialog
          job={conflictJob}
          onClose={() => setConflictJob(null)}
          onResolve={(decision, applyToRemaining) =>
            resolveConflict(conflictJob, decision, applyToRemaining)
          }
        />
      ) : null}
      {diagnosticPreview ? (
        <div
          aria-labelledby="diagnostic-dialog-title"
          aria-modal="true"
          className="paste-dialog-backdrop"
          role="dialog"
        >
          <section className="diagnostic-dialog">
            <h2 id="diagnostic-dialog-title">{t("diagnostic.title")}</h2>
            <p>{t("diagnostic.privacy")}</p>
            <textarea
              aria-label={t("diagnostic.preview")}
              readOnly
              value={diagnosticPreview.content}
            />
            <label>
              {t("diagnostic.destination")}
              <input
                onChange={(event) => setDiagnosticDestination(event.target.value)}
                placeholder={`/Users/you/Desktop/${diagnosticPreview.fileName}`}
                value={diagnosticDestination}
              />
            </label>
            <div className="paste-dialog-actions">
              <button onClick={() => setDiagnosticPreview(null)} type="button">
                {t("clipboard.cancel")}
              </button>
              <button
                disabled={!diagnosticDestination.trim()}
                onClick={exportDiagnostics}
                type="button"
              >
                {t("diagnostic.save")}
              </button>
            </div>
          </section>
        </div>
      ) : null}
      {pasteDraft ? (
        <div
          aria-label={t(
            pasteDraft.intent === "cut" ? "clipboard.confirmMove" : "clipboard.confirmCopy",
          )}
          aria-modal="true"
          className="paste-dialog-backdrop"
          role="dialog"
        >
          <section className="paste-dialog">
            <strong>
              {t(pasteDraft.intent === "cut" ? "clipboard.confirmMove" : "clipboard.confirmCopy")}
            </strong>
            <p>
              {t("clipboard.confirmDescription")
                .replace("{count}", String(pasteDraft.itemCount))
                .replace(
                  "{pane}",
                  t(pasteDraft.targetPane === "left" ? "pane.left" : "pane.right"),
                )}
            </p>
            <code>{pasteDraft.destinationDisplayPath}</code>
            <div className="paste-dialog-actions">
              <button
                onClick={() => {
                  void clipboardClient
                    .cancel(pasteDraft.draftId)
                    .finally(() => setPasteDraft(null));
                }}
                type="button"
              >
                {t("clipboard.cancel")}
              </button>
              <button
                onClick={() => {
                  const draftId = pasteDraft.draftId;
                  setPasteDraft(null);
                  void runOperation(() => clipboardClient.confirm(draftId));
                }}
                type="button"
              >
                {t("clipboard.confirm")}
              </button>
            </div>
          </section>
        </div>
      ) : null}
      {commandDialog ? (
        <div
          aria-label={t(
            commandDialog.type === "rename"
              ? "operation.renameTitle"
              : commandDialog.type === "createDirectory"
                ? "operation.newFolderTitle"
                : "operation.trashTitle",
          )}
          aria-modal="true"
          className="paste-dialog-backdrop"
          role="dialog"
        >
          <form
            className="paste-dialog"
            onSubmit={(event) => {
              event.preventDefault();
              submitCommand();
            }}
          >
            <strong>
              {t(
                commandDialog.type === "rename"
                  ? "operation.renameTitle"
                  : commandDialog.type === "createDirectory"
                    ? "operation.newFolderTitle"
                    : "operation.trashTitle",
              )}
            </strong>
            {commandDialog.type === "trash" ? (
              <p>
                {t("operation.trashDescription").replace(
                  "{count}",
                  String(commandDialog.entries.length),
                )}
              </p>
            ) : (
              <input
                aria-label={t("operation.name")}
                autoFocus
                onChange={(event) => setCommandName(event.target.value)}
                value={commandName}
              />
            )}
            <div className="paste-dialog-actions">
              <button onClick={() => setCommandDialog(null)} type="button">
                {t("clipboard.cancel")}
              </button>
              <button
                disabled={commandDialog.type !== "trash" && !commandName.trim()}
                type="submit"
              >
                {t("operation.confirm")}
              </button>
            </div>
          </form>
        </div>
      ) : null}
      {contextMenu ? (
        <div
          className="context-menu"
          onMouseLeave={() => setContextMenu(null)}
          role="menu"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          <button
            disabled={!hasTransferTarget || operationBusy}
            onClick={() => transfer("copy", [contextMenu.entry])}
            role="menuitem"
            title={!hasTransferTarget ? t("operation.disabled.destination") : undefined}
            type="button"
          >
            {t("operation.copy")}
          </button>
          <button
            disabled={!hasTransferTarget || operationBusy}
            onClick={() => transfer("move", [contextMenu.entry])}
            role="menuitem"
            title={!hasTransferTarget ? t("operation.disabled.destination") : undefined}
            type="button"
          >
            {t("operation.move")}
          </button>
          <button
            onClick={() => void setClipboard("copy", [contextMenu.entry])}
            role="menuitem"
            type="button"
          >
            {t("operation.copyClipboard")}
          </button>
          <button
            onClick={() => void setClipboard("cut", [contextMenu.entry])}
            role="menuitem"
            type="button"
          >
            {t("operation.cut")}
          </button>
          <button onClick={() => openRename([contextMenu.entry])} role="menuitem" type="button">
            {t("operation.rename")}
          </button>
          <button onClick={() => openTrash([contextMenu.entry])} role="menuitem" type="button">
            {t("operation.trash")}
          </button>
        </div>
      ) : null}
    </main>
  );
}

export default App;
