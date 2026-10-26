import { useModalFocus } from "./accessibility/useModalFocus";
import { BottomFeedback } from "./jobs/BottomFeedback";
import { OperationLogDialog } from "./jobs/OperationLogDialog";
import { useOperationEvents } from "./jobs/useOperationEvents";
import { DragInteraction, INTERNAL_DRAG_TYPE } from "./browser/dragInteraction";
import { FileContextMenu } from "./commands/FileContextMenu";
import {
  captureMenu,
  tauriContextMenuClient,
  type CapturedMenu,
  type ContextMenuClient,
  type ContextCommand,
} from "./commands/contextMenu";
import { useDirectoryTree } from "./browser/useDirectoryTree";
import {
  useCallback,
  useMemo,
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
import productIcon from "../src-tauri/icons/icon.png";
import {
  tauriDesktopMenuClient,
  useDesktopMenu,
  type DesktopMenuClient,
  type DesktopCommand,
  type DesktopMenuPresentation,
} from "./commands/desktopMenu";
import {
  captureCommandContext,
  isTextInput,
  type CommandContext,
  type CommandSource,
  type TransferTarget,
} from "./commands/context";
import {
  commandEnabled,
  newFolderParentEntry,
  createCommandRouter,
  dispatchWrite,
  dispatchClipboard,
  dispatchPaste,
  type FileCommand,
} from "./commands/dispatch";
import { useI18n } from "./i18n/I18nProvider";
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
  mergeFavorites,
  saveFavorites,
  type LocalFavorite,
  type SidebarFavorite,
} from "./browser/favorites";
import {
  tauriLocalLocationsClient,
  type LocalLocation,
  type LocalLocationsClient,
  type LocalLocationWarning,
} from "./browser/locationsApi";
import {
  tauriFinderFavoritesClient,
  type FinderFavoritesClient,
} from "./browser/finderFavoritesApi";
import { useFinderFavorites } from "./browser/useFinderFavorites";
import { useLocalLocations } from "./browser/useLocalLocations";
import { useLocationsChanged } from "./browser/useLocationsChanged";
import { VirtualFileList } from "./browser/VirtualFileList";
import { tauriUninstallClient } from "./browser/uninstallApi";
import { openFullDiskAccessSettings } from "./browser/finderAccessApi";
import { UninstallDialog } from "./uninstall/UninstallDialog";
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
  type LogStatus,
  type OperationLogSnapshot,
} from "./browser/operationApi";
import {
  tauriDiagnosticClient,
  type DiagnosticClient,
  type DiagnosticPreview,
} from "./browser/diagnosticApi";
import { tauriRecoveryClient, type RecoveryClient, type RecoveryItem } from "./browser/recoveryApi";
import type { EntrySnapshot } from "./ipc/pathReferences";
import { ConflictDialog } from "./jobs/ConflictDialog";
import { isActiveJob } from "./jobs/operationModel";

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
  locationWarnings?: LocalLocationWarning[];
  locations?: LocalLocation[];
  locationsLoading?: boolean;
  favorites?: SidebarFavorite[];
  finderFailed?: boolean;
  finderTimedOut?: boolean;
  finderUnresolved?: number;
  finderVisibilityAvailable?: boolean;
  onRefreshFavorites?: () => void;
  onOpenFinderSettings?: () => void;
  onRefreshLocations?: () => void;
  onAddFavorite?: (favorite: LocalFavorite) => void;
  onRemoveFavorite?: (path: string) => void;
  onDirectoryChange?: (listing: DirectoryListing | null) => void;
  onSelectionChange?: (entries: EntrySnapshot[]) => void;
  cutEntryKeys?: ReadonlySet<string>;
  onEntryContextMenu?: (
    entry: EntrySnapshot,
    x: number,
    y: number,
    selection?: EntrySnapshot[],
    expanded?: boolean,
  ) => void;
  onBlankContextMenu?: (x: number, y: number) => void;
  refreshToken?: number;
  dragToken?: string | null;
  validateDrop?: (target: EntrySnapshot | null, copy: boolean) => Promise<boolean>;
  onEntryDragEnd?: () => void;
  onEntryDragStart?: (entry: EntrySnapshot, selection?: EntrySnapshot[]) => string | void;
  onEntryDrop?: (entry: EntrySnapshot, copy: boolean) => void;
  onPaneDrop?: (copy: boolean) => void;
  onBrowseControls?: (controls: {
    refresh: () => void;
    toggleHidden: () => void;
    showHidden: boolean;
    open: (entry: EntrySnapshot) => void;
    folder: (entry: EntrySnapshot, expand: boolean) => void;
  }) => void;
}

function opensOnSingleClick(location: LocalLocation) {
  return (
    location.source === "standard" &&
    (location.kind === "desktop" || location.kind === "downloads" || location.kind === "documents")
  );
}

export function FilePane({
  active,
  client,
  id,
  onActivate,
  paneRef,
  monitor,
  locationWarnings = [],
  locations = [],
  locationsLoading = false,
  favorites = [],
  finderFailed = false,
  finderTimedOut = false,
  finderUnresolved = 0,
  finderVisibilityAvailable = true,
  onRefreshFavorites,
  onOpenFinderSettings,
  onRefreshLocations,
  onAddFavorite,
  onRemoveFavorite,
  onDirectoryChange,
  onSelectionChange,
  cutEntryKeys,
  onEntryContextMenu,
  onBlankContextMenu,
  refreshToken = 0,
  dragToken = null,
  validateDrop,
  onEntryDragEnd,
  onEntryDragStart,
  onEntryDrop,
  onPaneDrop,
  onBrowseControls,
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
  const [paneDropTarget, setPaneDropTarget] = useState<string | null>(null);
  const requestGeneration = useRef(0);
  const refreshRef = useRef<() => void>(() => undefined);
  const deferredRefresh = useRef(false);
  const pathInputRef = useRef<HTMLInputElement>(null);
  const paneName = t(id === "left" ? "pane.left" : "pane.right");
  const current = history[historyIndex] ?? null;
  const { tree, rows: treeRows } = useDirectoryTree(client, current, {
    pane: id,
    showHidden,
    sortField,
    sortDirection,
  });

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
    invalidateOnFailure = false,
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
      if (invalidateOnFailure && (code === "notFound" || code === "notDirectory"))
        tree.invalidateRoot();
    } finally {
      if (generation === requestGeneration.current) setLoading(false);
    }
  };

  const pushListing = (listing: DirectoryListing) => {
    deferredRefresh.current = false;
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

  const refreshCurrent = async () => {
    if (!current) return;
    if (loading) {
      deferredRefresh.current = true;
      return;
    }
    await runNavigation(() => client.reopen(current.directory, options()), replaceListing, true);
  };
  useEffect(() => {
    refreshRef.current = refreshCurrent;
  });
  useEffect(() => {
    if (!loading && deferredRefresh.current) {
      deferredRefresh.current = false;
      refreshRef.current();
    }
  }, [loading]);

  useEffect(() => {
    if (refreshToken > 0) {
      refreshRef.current();
      tree.refreshBranches();
    }
  }, [refreshToken, tree]);

  useEffect(() => {
    onBrowseControls?.({
      refresh: () => {
        void refreshCurrent();
        tree.refreshBranches();
      },
      toggleHidden: () => {
        const next = !showHidden;
        setShowHidden(next);
        updateView(next, sortField, sortDirection);
      },
      showHidden,
      open: openEntry,
      folder: (entry, expand) => {
        const row = treeRows.find((row) =>
          entry.treeKey
            ? row.key === entry.treeKey
            : row.entry.reference.sessionId === entry.reference.sessionId &&
              row.entry.reference.entryId === entry.reference.entryId,
        );
        if (!row || row.entry.kind !== "directory") return;
        if (expand && !row.expanded) void tree.expand(row.key);
        else if (!expand && row.expanded) tree.collapse(row.key);
      },
    });
  });

  const rootRefresh = useRef<() => Promise<void>>(async () => {});
  useEffect(() => {
    rootRefresh.current = refreshCurrent;
  });
  useEffect(() => {
    tree.setMonitor(
      monitor,
      () => rootRefresh.current(),
      (reason) => setBrowseError(reason === "resourceLimit" ? "resourceLimit" : "io"),
    );
    return () =>
      tree.setMonitor(
        undefined,
        async () => {},
        () => {},
      );
  }, [tree, monitor]);

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
  const [paneDrag] = useState(() => new DragInteraction());
  const dropValidation = useRef(validateDrop);
  useEffect(() => {
    dropValidation.current = validateDrop;
  }, [validateDrop]);
  useEffect(() => () => paneDrag.clear(), [paneDrag, dragToken, current?.directory.sessionId]);
  const [paneDropInvalid, setPaneDropInvalid] = useState<string | null>(null);
  const currentFavorite = current
    ? favorites.find((favorite) => favorite.path === current.displayPath)
    : undefined;
  const [selectedSidebar, setSelectedSidebar] = useState<string | null>(null);
  const [finderPermissionPrompt, setFinderPermissionPrompt] = useState(false);
  const standardLocations = locations.filter((location) => location.source === "standard");
  const positionLocations = locations.filter((location) => location.source !== "standard");
  const currentStandardLocation = current
    ? standardLocations.find((location) => location.path === current.displayPath)
    : undefined;
  const currentIsFavorite = Boolean(currentFavorite || currentStandardLocation);
  const currentIsAppFavorite = currentFavorite?.source === "app";
  const locationName = (location: LocalLocation) =>
    location.source === "standard" ? t(`location.${location.kind}` as MessageKey) : location.name;
  const locationIcon = (kind: LocalLocation["kind"]) => {
    switch (kind) {
      case "home":
        return "⌂";
      case "desktop":
      case "documents":
        return "▤";
      case "downloads":
        return "⇩";
      case "iCloudDrive":
        return "☁";
      case "cloudStorage":
        return "▣";
      case "localVolume":
        return "◫";
      case "externalVolume":
        return "▯";
      case "networkVolume":
        return "⇄";
    }
  };
  const openLocation = (location: LocalLocation) => {
    onActivate(id);
    void openPath(location.path);
  };
  const openFavorite = (favorite: LocalFavorite) => {
    onActivate(id);
    void openPath(favorite.path);
  };
  const activateOnKey = (event: ReactKeyboardEvent<HTMLButtonElement>, open: () => void) => {
    if (event.key !== "Enter" && !(event.metaKey && event.key === "ArrowDown")) return;
    event.preventDefault();
    onActivate(id);
    open();
  };
  const selectSidebar = (key: string) => {
    onActivate(id);
    setSelectedSidebar(key);
  };
  const unavailableSuffix = (available: boolean) =>
    available ? "" : ` — ${t("location.unavailable")}`;

  return (
    <section
      aria-label={paneName}
      className="file-pane"
      data-drop-target={Boolean(dragToken) && paneDropTarget === dragToken}
      data-drop-invalid={Boolean(dragToken) && paneDropInvalid === dragToken}
      onDragEnter={(event) => {
        if ((event.target as HTMLElement).closest(".file-row")) {
          paneDrag.clear();
          setPaneDropTarget(null);
          setPaneDropInvalid(null);
        }
      }}
      onDragLeave={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
          setPaneDropTarget(null);
          setPaneDropInvalid(null);
          paneDrag.clear();
        }
      }}
      onDragOver={(event) => {
        if (!dragToken || !current || !event.dataTransfer.types.includes(INTERNAL_DRAG_TYPE))
          return;
        event.preventDefault();
        const copy = event.altKey;
        const valid = paneDrag.target(
          `${dragToken}:${current.directory.sessionId}:${copy}`,
          () => dropValidation.current?.(null, copy) ?? Promise.resolve(false),
          (allowed) => {
            setPaneDropTarget(allowed ? dragToken : null);
            setPaneDropInvalid(allowed ? null : dragToken);
          },
        );
        event.dataTransfer.dropEffect = valid ? (copy ? "copy" : "move") : "none";
      }}
      onDrop={(event) => {
        if (!dragToken || !current || event.dataTransfer.getData(INTERNAL_DRAG_TYPE) !== dragToken)
          return;
        event.preventDefault();
        paneDrag.clear();
        setPaneDropTarget(null);
        setPaneDropInvalid(null);
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
          <img
            className="pane-product-icon"
            src={productIcon}
            alt=""
            aria-hidden="true"
            width="24"
            height="24"
            draggable={false}
          />
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
            onClick={() => {
              refreshRef.current();
              tree.refreshBranches();
            }}
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
          aria-label={
            currentStandardLocation
              ? t("location.standardFavorite")
              : currentFavorite && !currentIsAppFavorite
                ? t("location.finderFavorite")
                : currentIsFavorite
                  ? t("location.removeFavorite")
                  : t("location.addFavorite")
          }
          aria-pressed={currentIsFavorite}
          className="favorite-toggle"
          disabled={!current || (currentIsFavorite && !currentIsAppFavorite)}
          onClick={() => {
            if (!current) return;
            if (currentIsAppFavorite) onRemoveFavorite?.(current.displayPath);
            else if (!currentIsFavorite) onAddFavorite?.(favoriteForPath(current.displayPath));
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
        <aside
          aria-busy={locationsLoading}
          aria-label={t("location.sidebar")}
          className="location-sidebar"
        >
          <div className="sidebar-heading">
            <strong>{t("location.favorites")}</strong>
            <button
              aria-label={t("location.refreshFavorites")}
              className="sidebar-refresh"
              onClick={() => {
                onRefreshFavorites?.();
                if (!finderVisibilityAvailable) setFinderPermissionPrompt(true);
              }}
              type="button"
            >
              ↻
            </button>
          </div>
          <div className="location-list favorite-list">
            {standardLocations.map((location) => {
              const key = `location:${location.id}`;
              return (
                <button
                  aria-pressed={selectedSidebar === key}
                  data-unavailable={!location.available || undefined}
                  key={key}
                  onClick={(event) => {
                    selectSidebar(key);
                    if (opensOnSingleClick(location) && event.detail <= 1) {
                      openLocation(location);
                    }
                  }}
                  onDoubleClick={() => {
                    if (!opensOnSingleClick(location)) openLocation(location);
                  }}
                  onKeyDown={(event) => activateOnKey(event, () => openLocation(location))}
                  title={`${location.path}${unavailableSuffix(location.available)}`}
                  type="button"
                >
                  <span aria-hidden="true">{locationIcon(location.kind)}</span>
                  <span className="sidebar-label">{locationName(location)}</span>
                </button>
              );
            })}
            {favorites.map((favorite) => {
              const key = `favorite:${favorite.path}`;
              return (
                <div
                  className={
                    favorite.source === "app" ? "favorite-row" : "favorite-row system-favorite"
                  }
                  key={key}
                >
                  <button
                    aria-pressed={selectedSidebar === key}
                    data-unavailable={!favorite.available || undefined}
                    onClick={() => selectSidebar(key)}
                    onDoubleClick={() => openFavorite(favorite)}
                    onKeyDown={(event) => activateOnKey(event, () => openFavorite(favorite))}
                    title={`${favorite.path}${unavailableSuffix(favorite.available)}`}
                    type="button"
                  >
                    <span aria-hidden="true">★</span>
                    <span className="sidebar-label">{favorite.name}</span>
                  </button>
                  {favorite.source === "app" ? (
                    <button
                      aria-label={`${t("location.removeFavorite")}: ${favorite.name}`}
                      onClick={() => onRemoveFavorite?.(favorite.path)}
                      type="button"
                    >
                      ×
                    </button>
                  ) : null}
                </div>
              );
            })}
            {!standardLocations.length && !favorites.length ? (
              <span className="no-favorites">{t("location.noFavorites")}</span>
            ) : null}
          </div>
          {finderFailed ? (
            <div className="location-warning" role="status">
              {t("favorite.warning.unavailable")}
            </div>
          ) : null}
          {finderUnresolved > 0 ? (
            <div className="location-warning" role="status">
              {t("favorite.warning.unresolved").replace("{count}", String(finderUnresolved))}
            </div>
          ) : null}
          {finderTimedOut ? (
            <div className="location-warning" role="status">
              {t("favorite.warning.timeout")}
            </div>
          ) : null}
          {finderPermissionPrompt && !finderVisibilityAvailable ? (
            <div
              className="paste-dialog-backdrop"
              role="dialog"
              aria-modal="true"
              aria-labelledby="finder-permission-title"
            >
              <section className="paste-dialog">
                <header>
                  <h2 id="finder-permission-title">{t("finderPermission.title")}</h2>
                  <button
                    aria-label={t("feedback.dismiss")}
                    onClick={() => setFinderPermissionPrompt(false)}
                    type="button"
                  >
                    ×
                  </button>
                </header>
                <p>{t("finderPermission.message")}</p>
                <footer className="paste-dialog-actions">
                  <button onClick={() => setFinderPermissionPrompt(false)} type="button">
                    {t("finderPermission.cancel")}
                  </button>
                  <button
                    onClick={() => {
                      onOpenFinderSettings?.();
                      setFinderPermissionPrompt(false);
                    }}
                    type="button"
                  >
                    {t("finderPermission.openSettings")}
                  </button>
                </footer>
              </section>
            </div>
          ) : null}
          <div className="sidebar-heading">
            <strong>{t("location.positions")}</strong>
            <button
              aria-label={t("location.refresh")}
              className="sidebar-refresh"
              onClick={() => onRefreshLocations?.()}
              type="button"
            >
              ↻
            </button>
          </div>
          <div className="location-list">
            {positionLocations.map((location) => {
              const key = `location:${location.id}`;
              return (
                <button
                  aria-pressed={selectedSidebar === key}
                  data-unavailable={!location.available || undefined}
                  key={key}
                  onClick={(event) => {
                    selectSidebar(key);
                    if (opensOnSingleClick(location) && event.detail <= 1) {
                      openLocation(location);
                    }
                  }}
                  onDoubleClick={() => {
                    if (!opensOnSingleClick(location)) openLocation(location);
                  }}
                  onKeyDown={(event) => activateOnKey(event, () => openLocation(location))}
                  title={`${location.path}${unavailableSuffix(location.available)}`}
                  type="button"
                >
                  <span aria-hidden="true">{locationIcon(location.kind)}</span>
                  <span className="sidebar-label">{locationName(location)}</span>
                </button>
              );
            })}
            {!positionLocations.length ? (
              <span className="no-favorites">
                {locationsLoading ? t("location.loading") : t("location.noPositions")}
              </span>
            ) : null}
          </div>
          {locationWarnings.length ? (
            <div aria-live="polite" className="location-warning" role="status">
              {locationWarnings.map((warning) => (
                <span key={warning}>{t(`location.warning.${warning}` as MessageKey)}</span>
              ))}
            </div>
          ) : null}
        </aside>

        <div
          className="file-area"
          onContextMenu={(event) => {
            const target = event.target as HTMLElement;
            if (target.closest(".file-row, button, input, .file-table-header")) return;
            event.preventDefault();
            onActivate(id);
            onBlankContextMenu?.(event.clientX, event.clientY);
          }}
        >
          {browseError ? (
            <div className="navigation-error" role="alert">
              <span>
                <span aria-hidden="true">⚠ </span>
                {t(errorMessageKey(browseError))}
              </span>
              {current ? (
                <button
                  onClick={() => {
                    refreshRef.current();
                    tree.refreshBranches();
                  }}
                  type="button"
                >
                  {t("pane.retry")}
                </button>
              ) : null}
            </div>
          ) : null}
          {loading && !current ? (
            <div className="empty-pane-state" role="status">
              <strong>{t("pane.loading")}</strong>
            </div>
          ) : current ? (
            <VirtualFileList
              cutEntryKeys={cutEntryKeys}
              emptyState={
                current.entries.length === 0 ? (
                  <div className="empty-pane-state">
                    <span aria-hidden="true" className="empty-folder-icon">
                      ▱
                    </span>
                    <strong>{t("pane.emptyDirectory")}</strong>
                    <span>{t("pane.emptyDirectoryDescription")}</span>
                  </div>
                ) : undefined
              }
              entries={current.entries}
              loadIcon={(entry) => client.readEntryIcon?.(entry.reference) ?? Promise.resolve("")}
              pane={id}
              treeRows={treeRows}
              onToggleTree={(key) => {
                const row = treeRows.find((row) => row.key === key);
                if (row?.expanded) tree.collapse(key);
                else void tree.expand(key);
              }}
              onHoverExpand={(key) => {
                void tree.expand(key);
              }}
              onRetryTree={(key) => tree.retry(key)}
              onOpen={openEntry}
              onEntryContextMenu={(entry, x, y, selection) =>
                onEntryContextMenu?.(
                  entry,
                  x,
                  y,
                  selection,
                  treeRows.find(
                    (row) =>
                      row.entry.reference.sessionId === entry.reference.sessionId &&
                      row.entry.reference.entryId === entry.reference.entryId,
                  )?.expanded ?? false,
                )
              }
              dragToken={dragToken}
              validateDrop={(entry, copy) => validateDrop?.(entry, copy) ?? Promise.resolve(false)}
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
          ) : browseError ? (
            <div className="empty-pane-state error-state">
              <strong>{t("error.title")}</strong>
            </div>
          ) : (
            <div className="empty-pane-state">
              <span aria-hidden="true" className="empty-folder-icon">
                ▱
              </span>
              <strong>{t("pane.emptyTitle")}</strong>
              <span>{t("pane.emptyDescription")}</span>
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
  contextMenuClient?: ContextMenuClient;
  desktopMenuClient?: DesktopMenuClient;
  clipboardClient?: ClipboardClient;
  directoryClient?: DirectoryClient;
  finderFavoritesClient?: FinderFavoritesClient;
  locationsClient?: LocalLocationsClient;
  monitor?: DirectoryMonitor;
  operationClient?: OperationClient;
  diagnosticClient?: DiagnosticClient;
  recoveryClient?: RecoveryClient;
}

function App({
  contextMenuClient = tauriContextMenuClient,
  desktopMenuClient = tauriDesktopMenuClient,
  clipboardClient = tauriClipboardClient,
  directoryClient = tauriDirectoryClient,
  finderFavoritesClient = tauriFinderFavoritesClient,
  locationsClient = tauriLocalLocationsClient,
  monitor = tauriDirectoryMonitor,
  operationClient = tauriOperationClient,
  diagnosticClient = tauriDiagnosticClient,
  recoveryClient = tauriRecoveryClient,
}: AppProps) {
  const { language, setLanguage, t } = useI18n();
  const [textEditing, setTextEditing] = useState(false);
  const [paneHidden, setPaneHidden] = useState({ left: false, right: false });
  const browseControls = useRef<
    Partial<
      Record<
        PaneId,
        {
          refresh: () => void;
          toggleHidden: () => void;
          open: (entry: EntrySnapshot) => void;
          folder: (entry: EntrySnapshot, expand: boolean) => void;
        }
      >
    >
  >({});
  const [activePane, setActivePane] = useState<PaneId>("left");
  const [splitPercent, setSplitPercent] = useState(50);
  const {
    locations,
    warnings: locationWarnings,
    loading: locationsLoading,
    refresh: refreshLocations,
  } = useLocalLocations(locationsClient);
  const [favorites, setFavorites] = useState<LocalFavorite[]>(() => loadFavorites());
  const finderFavorites = useFinderFavorites(finderFavoritesClient);
  const refreshFinderFavorites = finderFavorites.refresh;
  useLocationsChanged(
    useCallback(() => {
      refreshLocations();
      refreshFinderFavorites();
    }, [refreshLocations, refreshFinderFavorites]),
  );
  const standardPaths = useMemo(
    () =>
      new Set(
        locations
          .filter((location) => location.source === "standard")
          .map((location) => location.path),
      ),
    [locations],
  );
  const mergedFavorites = useMemo(
    () =>
      mergeFavorites(
        finderFavorites.favorites.filter((favorite) => !standardPaths.has(favorite.path)),
        favorites.filter((favorite) => !standardPaths.has(favorite.path)),
      ),
    [finderFavorites.favorites, favorites, standardPaths],
  );
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
  const [logView, setLogView] = useState<{
    data: OperationLogSnapshot;
    operationId?: string;
  } | null>(null);
  const [uninstallOpen, setUninstallOpen] = useState(false);
  const [logStatus, setLogStatus] = useState<LogStatus[]>([]);
  const [connectionFailed, setConnectionFailed] = useState(false);
  const [deferredConflicts, setDeferredConflicts] = useState<Set<number>>(new Set());
  const [conflictJob, setConflictJob] = useState<OperationJob | null>(null);
  const [diagnosticPreview, setDiagnosticPreview] = useState<DiagnosticPreview | null>(null);
  const [diagnosticDestination, setDiagnosticDestination] = useState("");
  const [recoveryItems, setRecoveryItems] = useState<RecoveryItem[]>([]);
  const [interruptedTasks, setInterruptedTasks] = useState(
    () => window.localStorage.getItem(ACTIVE_TASKS_STORAGE_KEY) !== null,
  );
  const [dragState, setDragState] = useState<{ token: string; context: CommandContext } | null>(
    null,
  );
  const dragSessionRef = useRef<typeof dragState>(null);
  const clearDrag = useCallback(() => {
    dragSessionRef.current = null;
    setDragState(null);
  }, []);
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") clearDrag();
    };
    window.addEventListener("keydown", escape);
    window.addEventListener("blur", clearDrag);
    return () => {
      window.removeEventListener("keydown", escape);
      window.removeEventListener("blur", clearDrag);
    };
  }, [clearDrag]);
  const [refreshToken, setRefreshToken] = useState(0);
  const [commandDialog, setCommandDialog] = useState<{
    type: "rename" | "createDirectory" | "trash";
    context: CommandContext;
  } | null>(null);
  const [commandName, setCommandName] = useState("");
  const [contextMenu, setContextMenu] = useState<CapturedMenu | null>(null);
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
    void recoveryClient
      .list()
      .then(setRecoveryItems)
      .catch(() => setRecoveryItems([]));
  }, [recoveryClient]);

  useEffect(() => {
    const closeTopDialog = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      if (logView) {
        setLogView(null);
      } else if (diagnosticPreview) {
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
  }, [clipboardClient, commandDialog, diagnosticPreview, pasteDraft, logView]);

  useOperationEvents(
    operationClient,
    (snapshot) => {
      const next = snapshot.jobs;
      setJobs(next);
      setLogStatus([snapshot.logStatus, snapshot.recoveryLogStatus]);
      setConnectionFailed(false);
      let terminalChanged = false;
      for (const job of next) {
        if (!isActiveJob(job) && !terminalJobsSeen.current.has(job.jobId)) {
          terminalJobsSeen.current.add(job.jobId);
          terminalChanged = true;
        }
      }
      if (terminalChanged) setRefreshToken((value) => value + 1);
    },
    () => setConnectionFailed(true),
  );

  useEffect(() => {
    const active = jobs.filter(isActiveJob);
    if (active.length)
      window.localStorage.setItem(
        ACTIVE_TASKS_STORAGE_KEY,
        JSON.stringify(active.map((job) => job.jobId)),
      );
    else window.localStorage.removeItem(ACTIVE_TASKS_STORAGE_KEY);
  }, [jobs]);

  const updateFavorites = (next: LocalFavorite[]) => {
    setFavorites(next);
    saveFavorites(next);
  };

  const addFavorite = (favorite: LocalFavorite) => {
    if (
      mergedFavorites.some((existing) => existing.path === favorite.path) ||
      locations.some(
        (location) => location.source === "standard" && location.path === favorite.path,
      )
    )
      return;
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

  const workspaceStyle = {
    "--left-pane-width": `${splitPercent}fr`,
    "--right-pane-width": `${100 - splitPercent}fr`,
  } as CSSProperties;
  const cutEntryKeys = new Set(
    clipboardState?.cutEntries.map((reference) => `${reference.sessionId}:${reference.entryId}`) ??
      [],
  );

  const getContext = (
    source: CommandSource = "toolbar",
    pane: PaneId = activePane,
    entries: readonly EntrySnapshot[] = paneSelections[pane],
    target?: TransferTarget | null,
  ) => captureCommandContext(source, pane, paneListings, entries, target);

  const setClipboard = async (intent: "copy" | "cut", context = getContext()) => {
    if (!commandEnabled(intent, context)) return;
    try {
      const next = await dispatchClipboard(clipboardClient, intent, context);
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
    setJobs((current) =>
      current.some((item) => item.jobId === job.jobId) ? current : [job, ...current],
    );
    if (!isActiveJob(job)) setRefreshToken((value) => value + 1);
  };
  const openLog = (operationId?: string) => {
    void (
      operationClient.getLog?.() ??
      Promise.resolve({
        events: [],
        status: { persistenceEnabled: false, failedWrites: 0, lastError: null },
        recoveryStatus: { persistenceEnabled: false, failedWrites: 0, lastError: null },
      })
    )
      .then((data) => setLogView({ data, operationId }))
      .catch(() => setClipboardNotice(t("feedback.logFailed")));
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
        setJobs((current) =>
          current.map((item) => (item.jobId === jobId && isActiveJob(item) ? job : item)),
        ),
      )
      .catch(() => setClipboardNotice(t("operation.failed")));
  };

  const resolveConflict = (
    job: OperationJob,
    decision: "skip" | "replace" | "keepBoth" | "cancel",
    applyToRemaining: boolean,
  ) => {
    if (operationBusy) return;
    setOperationBusy(true);
    setConflictJob(null);
    setDeferredConflicts((current) => new Set(current).add(job.jobId));
    void operationClient
      .resolveConflict(job.jobId, decision, applyToRemaining)
      .then((next) => {
        setJobs((current) => current.filter((item) => item.jobId !== job.jobId));
        finishOperation(next);
      })
      .catch(() => {
        setConflictJob(job);
        setClipboardNotice(t("operation.failed"));
      })
      .finally(() => setOperationBusy(false));
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

  const transfer = (kind: "copy" | "move", context = getContext()) => {
    const command = kind === "copy" ? "copyToOther" : "moveToOther";
    if (!commandEnabled(command, context, operationBusy)) return;
    void runOperation(() => dispatchWrite(operationClient, command, context));
  };

  const beginDrag = (pane: PaneId, entry: EntrySnapshot, selection?: EntrySnapshot[]) => {
    activatePane(pane);
    const selected = paneSelections[pane].some(
      (item) =>
        item.reference.sessionId === entry.reference.sessionId &&
        item.reference.entryId === entry.reference.entryId,
    );
    const session = {
      token: crypto.randomUUID(),
      context: getContext("drag", pane, selection ?? (selected ? paneSelections[pane] : [entry])),
    };
    dragSessionRef.current = session;
    setDragState(session);
    return session.token;
  };
  const dragContext = (
    pane: PaneId,
    entry: EntrySnapshot | null,
    delivered = false,
  ): CommandContext | null => {
    const session = dragSessionRef.current;
    const root = paneListings[pane];
    if (!session || operationBusy || !root || (!delivered && entry?.isReadOnly)) return null;
    if (
      !delivered &&
      entry &&
      session.context.entries.some(
        (source) =>
          (source.treeKey && source.treeKey === entry.treeKey) ||
          (source.reference.sessionId === entry.reference.sessionId &&
            source.reference.entryId === entry.reference.entryId),
      )
    )
      return null;
    if (
      !delivered &&
      !entry &&
      session.context.entries.some(
        (source) => source.reference.sessionId === root.directory.sessionId,
      )
    )
      return null;
    if (entry && !(entry.kind === "directory" || entry.symlinkTarget === "directory")) return null;
    return {
      ...session.context,
      target: entry
        ? { kind: "entry", pane, entry: entry.reference }
        : { kind: "directory", pane, directory: root.directory },
    };
  };
  const validateDrag = async (pane: PaneId, entry: EntrySnapshot | null, copy: boolean) => {
    const token = dragSessionRef.current?.token;
    const context = dragContext(pane, entry);
    if (!context) return false;
    try {
      await operationClient.validateTransfer?.(
        copy ? "copy" : "move",
        context.entries.map((item) => item.reference),
        context.target!,
      );
      return Boolean(token && dragSessionRef.current?.token === token);
    } catch {
      return false;
    }
  };
  const drop = async (pane: PaneId, entry: EntrySnapshot | null, copy: boolean) => {
    const context = dragContext(pane, entry, true);
    clearDrag();
    if (!context) {
      setClipboardNotice(t("operation.invalidDrop"));
      return;
    }
    // A delivered drop owns its captured context; the following native dragend only clears UI.
    setOperationBusy(true);
    try {
      finishOperation(
        await dispatchWrite(operationClient, copy ? "copyToOther" : "moveToOther", context),
      );
    } catch {
      setClipboardNotice(t("operation.invalidDrop"));
    } finally {
      setOperationBusy(false);
    }
  };

  const openRename = (context = getContext()) => {
    if (!commandEnabled("rename", context, operationBusy)) return;
    setCommandName(context.entries[0].displayName);
    setCommandDialog({ type: "rename", context });
    setContextMenu(null);
  };

  const openCreateDirectory = (context = getContext()) => {
    if (!commandEnabled("newFolder", context, operationBusy)) return;
    setCommandName("");
    setCommandDialog({ type: "createDirectory", context });
    setContextMenu(null);
  };

  const openTrash = (context = getContext()) => {
    if (!commandEnabled("trash", context, operationBusy)) return;
    setCommandDialog({ type: "trash", context });
    setContextMenu(null);
  };

  const submitCommand = () => {
    if (!commandDialog) return;
    const dialog = commandDialog;
    setCommandDialog(null);
    const command = dialog.type === "createDirectory" ? "newFolder" : dialog.type;
    if (command !== "trash" && !commandName.trim()) return;
    void runOperation(() => dispatchWrite(operationClient, command, dialog.context, commandName));
  };

  const preparePaste = async (context = getContext()) => {
    try {
      setPasteDraft(
        await dispatchPaste(clipboardClient, context, clipboardState?.sourcePane ?? undefined),
      );
      setContextMenu(null);
      setClipboardNotice(null);
    } catch {
      setClipboardNotice(
        t(context.directory || context.target ? "clipboard.error" : "clipboard.noTarget"),
      );
    }
  };

  const routeCommand = createCommandRouter(
    {
      transfer,
      clipboard: (intent, context) => {
        void setClipboard(intent, context);
      },
      paste: (context) => {
        void preparePaste(context);
      },
      dialog: (kind, context) => {
        if (kind === "rename") openRename(context);
        else if (kind === "newFolder") openCreateDirectory(context);
        else openTrash(context);
      },
    },
    operationBusy,
  );
  const dispatchCommand = (command: FileCommand, context = getContext()) =>
    routeCommand(command, context);

  const menuContext = useMemo(
    () => captureCommandContext("menu", activePane, paneListings, paneSelections[activePane]),
    [activePane, paneListings, paneSelections],
  );
  const pendingConflict = jobs.find(
    (job) => job.state === "awaitingDecision" && job.conflict && !deferredConflicts.has(job.jobId),
  );
  const visibleConflict =
    conflictJob ??
    (!commandDialog && !pasteDraft && !diagnosticPreview && !logView && !operationBusy
      ? pendingConflict
      : null);
  const modalOpen = Boolean(
    commandDialog || pasteDraft || diagnosticPreview || visibleConflict || logView || uninstallOpen,
  );
  const menuPresentation = useMemo<DesktopMenuPresentation>(() => {
    const enabledCommands: DesktopCommand[] = ["languageEnglish", "languageChinese"];
    if (!modalOpen) enabledCommands.push("diagnostics", "operationLog", "uninstall");
    if (!textEditing && !modalOpen) {
      for (const command of [
        "copyToOther",
        "moveToOther",
        "copy",
        "cut",
        "paste",
        "rename",
        "newFolder",
        "trash",
      ] as const) {
        if (commandEnabled(command, menuContext, operationBusy)) enabledCommands.push(command);
      }
      if (menuContext.directory) enabledCommands.push("refresh", "toggleHidden");
    }
    return { language, enabledCommands, textEditing, showHidden: paneHidden[activePane] };
  }, [language, textEditing, modalOpen, menuContext, operationBusy, paneHidden, activePane]);
  const desktopMenu = useDesktopMenu(
    desktopMenuClient,
    menuPresentation,
    menuContext,
    (command, context) => {
      if (command === "languageEnglish" || command === "languageChinese") {
        setLanguage(command === "languageEnglish" ? "en" : "zh-CN");
      } else if (command === "diagnostics") openDiagnostics();
      else if (command === "operationLog") openLog();
      else if (command === "uninstall") setUninstallOpen(true);
      else if (command === "refresh") browseControls.current[context.pane]?.refresh();
      else if (command === "toggleHidden") browseControls.current[context.pane]?.toggleHidden();
      else if (!isTextInput(document.activeElement) && !modalOpen)
        dispatchCommand(command, context);
    },
  );
  const updateBrowseControls = (
    pane: PaneId,
    controls: {
      refresh: () => void;
      toggleHidden: () => void;
      showHidden: boolean;
      open: (entry: EntrySnapshot) => void;
      folder: (entry: EntrySnapshot, expand: boolean) => void;
    },
  ) => {
    browseControls.current[pane] = controls;
    setPaneHidden((current) =>
      current[pane] === controls.showHidden ? current : { ...current, [pane]: controls.showHidden },
    );
  };

  useModalFocus(
    !(visibleConflict || logView) && (commandDialog ?? pasteDraft ?? diagnosticPreview),
  );

  const handleShortcut = (event: ReactKeyboardEvent<HTMLElement>) => {
    if (event.defaultPrevented || isTextInput(event.target) || modalOpen) return;
    if (event.altKey || event.ctrlKey) return;
    let command: FileCommand | undefined;
    if (!event.metaKey && !event.shiftKey) {
      command = ({ F5: "copyToOther", F6: "moveToOther", F2: "rename" } as const)[
        event.key as "F5" | "F6" | "F2"
      ];
    } else if (event.metaKey) {
      const key = event.key.toLowerCase();
      if (event.shiftKey && key === "n") command = "newFolder";
      else if (!event.shiftKey)
        command = ({ c: "copy", x: "cut", v: "paste", backspace: "trash" } as const)[
          key as "c" | "x" | "v" | "backspace"
        ];
    }
    if (!command) return;
    if (desktopMenu.active) return;
    event.preventDefault();
    dispatchCommand(command, getContext("keyboard"));
  };

  return (
    <main
      aria-label={t("app.name")}
      className="app-shell"
      onFocusCapture={(event) => setTextEditing(isTextInput(event.target))}
      onPointerDownCapture={(event) => setTextEditing(isTextInput(event.target))}
      onKeyDown={(event) => {
        handleShortcut(event);
        if (event.defaultPrevented) return;
        if (event.key !== "Tab" || event.altKey || !event.ctrlKey || event.metaKey) return;
        event.preventDefault();
        activatePane(activePane === "left" ? "right" : "left", true);
      }}
    >
      <div className="workspace" ref={workspaceRef} style={workspaceStyle}>
        <FilePane
          active={activePane === "left"}
          client={directoryClient}
          favorites={mergedFavorites}
          finderFailed={finderFavorites.failed}
          finderTimedOut={finderFavorites.timedOut}
          finderUnresolved={finderFavorites.unresolved}
          finderVisibilityAvailable={finderFavorites.visibilityAvailable}
          onOpenFinderSettings={() => void openFullDiskAccessSettings()}
          id="left"
          onBrowseControls={(controls) => updateBrowseControls("left", controls)}
          locationWarnings={locationWarnings}
          locations={locations}
          locationsLoading={locationsLoading}
          monitor={monitor}
          onRefreshFavorites={refreshFinderFavorites}
          onRefreshLocations={refreshLocations}
          onAddFavorite={addFavorite}
          onActivate={activatePane}
          onRemoveFavorite={removeFavorite}
          onDirectoryChange={onLeftDirectoryChange}
          onSelectionChange={onLeftSelectionChange}
          cutEntryKeys={cutEntryKeys}
          dragToken={dragState?.token}
          validateDrop={(entry, copy) => validateDrag("left", entry, copy)}
          onEntryContextMenu={(entry, x, y, selection, expanded) => {
            activatePane("left");
            const selected = paneSelections.left.some(
              (item) =>
                item.reference.sessionId === entry.reference.sessionId &&
                item.reference.entryId === entry.reference.entryId,
            );
            setContextMenu(
              captureMenu(
                getContext(
                  "contextMenu",
                  "left",
                  selection ?? (selected ? paneSelections.left : [entry]),
                ),
                x,
                y,
                expanded,
              ),
            );
          }}
          onBlankContextMenu={(x, y) => {
            activatePane("left");
            setContextMenu(captureMenu(getContext("blankMenu", "left", []), x, y));
          }}
          onEntryDragEnd={clearDrag}
          onEntryDragStart={(entry, selection) => beginDrag("left", entry, selection)}
          onEntryDrop={(target, copy) => {
            void drop("left", target, copy);
          }}
          onPaneDrop={(copy) => {
            void drop("left", null, copy);
          }}
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
          favorites={mergedFavorites}
          finderFailed={finderFavorites.failed}
          finderTimedOut={finderFavorites.timedOut}
          finderUnresolved={finderFavorites.unresolved}
          finderVisibilityAvailable={finderFavorites.visibilityAvailable}
          onOpenFinderSettings={() => void openFullDiskAccessSettings()}
          id="right"
          onBrowseControls={(controls) => updateBrowseControls("right", controls)}
          locationWarnings={locationWarnings}
          locations={locations}
          locationsLoading={locationsLoading}
          monitor={monitor}
          onRefreshFavorites={refreshFinderFavorites}
          onRefreshLocations={refreshLocations}
          onAddFavorite={addFavorite}
          onActivate={activatePane}
          onRemoveFavorite={removeFavorite}
          onDirectoryChange={onRightDirectoryChange}
          onSelectionChange={onRightSelectionChange}
          cutEntryKeys={cutEntryKeys}
          dragToken={dragState?.token}
          validateDrop={(entry, copy) => validateDrag("right", entry, copy)}
          onEntryContextMenu={(entry, x, y, selection, expanded) => {
            activatePane("right");
            const selected = paneSelections.right.some(
              (item) =>
                item.reference.sessionId === entry.reference.sessionId &&
                item.reference.entryId === entry.reference.entryId,
            );
            setContextMenu(
              captureMenu(
                getContext(
                  "contextMenu",
                  "right",
                  selection ?? (selected ? paneSelections.right : [entry]),
                ),
                x,
                y,
                expanded,
              ),
            );
          }}
          onBlankContextMenu={(x, y) => {
            activatePane("right");
            setContextMenu(captureMenu(getContext("blankMenu", "right", []), x, y));
          }}
          onEntryDragEnd={clearDrag}
          onEntryDragStart={(entry, selection) => beginDrag("right", entry, selection)}
          onEntryDrop={(target, copy) => {
            void drop("right", target, copy);
          }}
          onPaneDrop={(copy) => {
            void drop("right", null, copy);
          }}
          paneRef={rightPaneRef}
          refreshToken={refreshToken}
        />
      </div>
      {desktopMenu.failed ? (
        <div role="alert" className="navigation-error">
          {t("menu.error")}
        </div>
      ) : null}
      <BottomFeedback
        jobs={jobs}
        logStatus={logStatus}
        connectionFailed={connectionFailed}
        notice={clipboardNotice}
        onDismissNotice={() => setClipboardNotice(null)}
        onCancel={cancelJob}
        onReviewConflict={setConflictJob}
        onLog={openLog}
        onDiagnostics={openDiagnostics}
      />
      {logView && (
        <OperationLogDialog
          data={logView.data}
          operationId={logView.operationId}
          onClose={() => setLogView(null)}
          onDiagnostics={openDiagnostics}
        />
      )}

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
      {visibleConflict ? (
        <ConflictDialog
          key={visibleConflict.jobId}
          job={visibleConflict}
          busy={operationBusy}
          onClose={() => {
            setDeferredConflicts((current) => new Set(current).add(visibleConflict.jobId));
            setConflictJob(null);
          }}
          onResolve={(decision, applyToRemaining) =>
            resolveConflict(visibleConflict, decision, applyToRemaining)
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
            {commandDialog.type === "createDirectory" &&
            newFolderParentEntry(commandDialog.context) ? (
              <p>
                {t("operation.newFolderInsideDescription").replace(
                  "{name}",
                  newFolderParentEntry(commandDialog.context)!.displayName,
                )}
              </p>
            ) : null}
            {commandDialog.type === "trash" ? (
              <p>
                {t("operation.trashDescription").replace(
                  "{count}",
                  String(commandDialog.context.entries.length),
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
      {uninstallOpen ? (
        <UninstallDialog
          client={tauriUninstallClient}
          onClose={() => setUninstallOpen(false)}
          open
        />
      ) : null}
      {contextMenu ? (
        <FileContextMenu
          key={contextMenu.id}
          menu={contextMenu}
          client={contextMenuClient}
          busy={operationBusy}
          showHidden={paneHidden[contextMenu.context.pane]}
          onClose={() => setContextMenu(null)}
          onChoose={(command: ContextCommand) => {
            const menu = contextMenu;
            setContextMenu(null);
            const controls = browseControls.current[menu.context.pane];
            if (command === "open") controls?.open(menu.context.entries[0]);
            else if (command === "toggleFolder")
              controls?.folder(menu.context.entries[0], !menu.expanded);
            else if (command === "refresh") controls?.refresh();
            else if (command === "toggleHidden") controls?.toggleHidden();
            else if (command === "operationLog") openLog();
            else if (command === "diagnostics") openDiagnostics();
            else dispatchCommand(command, menu.context);
          }}
        />
      ) : null}
    </main>
  );
}

export default App;
