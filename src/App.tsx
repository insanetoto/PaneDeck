import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type CSSProperties,
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
import type { EntrySnapshot } from "./ipc/pathReferences";

const supportedLanguages: Language[] = ["zh-CN", "en"];
const MIN_SPLIT_PERCENT = 28;
const MAX_SPLIT_PERCENT = 72;

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
  const requestGeneration = useRef(0);
  const refreshRef = useRef<() => void>(() => undefined);
  const pathInputRef = useRef<HTMLInputElement>(null);
  const paneName = t(id === "left" ? "pane.left" : "pane.right");
  const current = history[historyIndex] ?? null;

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
              <span>{t(errorMessageKey(browseError))}</span>
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
              entries={current.entries}
              onOpen={openEntry}
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
  directoryClient?: DirectoryClient;
  locationsClient?: LocalLocationsClient;
  monitor?: DirectoryMonitor;
}

function App({
  directoryClient = tauriDirectoryClient,
  locationsClient = tauriLocalLocationsClient,
  monitor = tauriDirectoryMonitor,
}: AppProps) {
  const { language, setLanguage, t } = useI18n();
  const [activePane, setActivePane] = useState<PaneId>("left");
  const [splitPercent, setSplitPercent] = useState(50);
  const [locations, setLocations] = useState<LocalLocation[]>([]);
  const [favorites, setFavorites] = useState<LocalFavorite[]>(() => loadFavorites());
  const workspaceRef = useRef<HTMLDivElement>(null);
  const leftPaneRef = useRef<HTMLElement>(null);
  const rightPaneRef = useRef<HTMLElement>(null);
  const dragging = useRef(false);

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

  return (
    <main
      aria-label={t("app.name")}
      className="app-shell"
      onKeyDown={(event) => {
        if (event.key !== "Tab" || event.altKey || event.ctrlKey || event.metaKey) return;
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
          paneRef={leftPaneRef}
        />
        <div
          aria-label={t("pane.resize")}
          aria-orientation="vertical"
          aria-valuemax={MAX_SPLIT_PERCENT}
          aria-valuemin={MIN_SPLIT_PERCENT}
          aria-valuenow={Math.round(splitPercent)}
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
          role="separator"
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
          paneRef={rightPaneRef}
        />
      </div>
    </main>
  );
}

export default App;
