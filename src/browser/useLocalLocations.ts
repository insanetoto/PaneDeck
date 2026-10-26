import { useCallback, useEffect, useRef, useState } from "react";
import type { LocalLocation, LocalLocationsClient, LocalLocationWarning } from "./locationsApi";

const FOCUS_REFRESH_INTERVAL_MS = 30_000;

interface LocationState {
  locations: LocalLocation[];
  warnings: LocalLocationWarning[];
  loading: boolean;
}

const initialState: LocationState = { locations: [], warnings: [], loading: true };

/**
 * One shared discovery snapshot for both panes. Requests are generation-bound:
 * a slow response can never replace a newer one, and focus refresh is throttled
 * so activating the window does not start an unbounded stream of probes.
 */
export function useLocalLocations(client: LocalLocationsClient) {
  const [state, setState] = useState<LocationState>(initialState);
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
          setState({ locations: result.locations, warnings: result.warnings, loading: false });
        })
        .catch(() => {
          if (!mounted.current || request !== generation.current) return;
          setState((current) => ({
            ...current,
            loading: false,
            warnings: current.warnings.includes("refreshFailed")
              ? current.warnings
              : [...current.warnings, "refreshFailed"],
          }));
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
