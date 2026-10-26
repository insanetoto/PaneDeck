import { useCallback, useEffect, useRef, useState } from "react";
import type { FinderFavorite, FinderFavoritesClient } from "./finderFavoritesApi";

const FOCUS_REFRESH_INTERVAL_MS = 30_000;

interface FinderState {
  favorites: FinderFavorite[];
  unresolved: number;
  failed: boolean;
  timedOut: boolean;
  visibilityAvailable: boolean;
  loading: boolean;
}

const initialState: FinderState = {
  favorites: [],
  unresolved: 0,
  failed: false,
  timedOut: false,
  visibilityAvailable: false,
  loading: true,
};

/**
 * Read-only Finder favorites snapshot. A failed or timed-out refresh keeps the
 * last list; the app's own favorites are merged separately and never removed.
 */
export function useFinderFavorites(client: FinderFavoritesClient) {
  const [state, setState] = useState<FinderState>(initialState);
  const generation = useRef(0);
  const lastStartedAt = useRef(0);
  const mounted = useRef(true);

  const load = useCallback(
    (refresh: boolean, throttle: boolean) => {
      const now = Date.now();
      if (throttle && now - lastStartedAt.current < FOCUS_REFRESH_INTERVAL_MS) return;
      lastStartedAt.current = now;
      const request = ++generation.current;
      setState((current) => ({ ...current, loading: true }));
      void client
        .list(refresh)
        .then((result) => {
          if (!mounted.current || request !== generation.current) return;
          setState({
            favorites: result.favorites,
            unresolved: result.unresolved,
            failed: result.failed,
            timedOut: result.timedOut,
            visibilityAvailable: result.visibilityAvailable ?? false,
            loading: false,
          });
        })
        .catch(() => {
          if (!mounted.current || request !== generation.current) return;
          setState((current) => ({ ...current, loading: false, failed: true }));
        });
    },
    [client],
  );

  useEffect(() => {
    mounted.current = true;
    load(false, false);
    const onFocus = () => load(true, true);
    const onVisibility = () => {
      if (document.visibilityState === "visible") load(true, true);
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      mounted.current = false;
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [load]);

  const refresh = useCallback(() => load(true, false), [load]);
  return { ...state, refresh };
}
