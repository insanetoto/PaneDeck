import type { FinderFavorite } from "./finderFavoritesApi";

export const FAVORITES_STORAGE_KEY = "panedeck.local-favorites.v1";
const MAX_FAVORITES = 100;

export interface LocalFavorite {
  name: string;
  path: string;
}

/** A sidebar row with an explicit, read-only-aware source. */
export interface SidebarFavorite extends LocalFavorite {
  source: "app" | "finder";
  available: boolean;
}

/**
 * Finder order and names win; app-only entries append. A path present in both
 * remains removable as an app record while keeping the Finder name.
 */
export function mergeFavorites(
  finder: readonly FinderFavorite[],
  app: readonly LocalFavorite[],
): SidebarFavorite[] {
  const appPaths = new Set(app.map((favorite) => favorite.path));
  const seen = new Set<string>();
  const merged: SidebarFavorite[] = [];
  for (const favorite of finder) {
    if (seen.has(favorite.path)) continue;
    seen.add(favorite.path);
    merged.push({
      name: favorite.name,
      path: favorite.path,
      source: appPaths.has(favorite.path) ? "app" : "finder",
      available: favorite.available,
    });
  }
  for (const favorite of app) {
    if (seen.has(favorite.path)) continue;
    seen.add(favorite.path);
    merged.push({ ...favorite, source: "app", available: true });
  }
  return merged;
}

export function loadFavorites(storage: Storage = window.localStorage): LocalFavorite[] {
  try {
    const parsed: unknown = JSON.parse(storage.getItem(FAVORITES_STORAGE_KEY) ?? "[]");
    if (!Array.isArray(parsed)) return [];
    const unique = new Map<string, LocalFavorite>();
    for (const candidate of parsed) {
      if (
        typeof candidate === "object" &&
        candidate !== null &&
        "path" in candidate &&
        "name" in candidate &&
        typeof candidate.path === "string" &&
        candidate.path.startsWith("/") &&
        typeof candidate.name === "string" &&
        candidate.name.trim()
      ) {
        unique.set(candidate.path, { path: candidate.path, name: candidate.name.trim() });
      }
      if (unique.size >= MAX_FAVORITES) break;
    }
    return [...unique.values()];
  } catch {
    return [];
  }
}

export function saveFavorites(
  favorites: readonly LocalFavorite[],
  storage: Storage = window.localStorage,
) {
  storage.setItem(FAVORITES_STORAGE_KEY, JSON.stringify(favorites.slice(0, MAX_FAVORITES)));
}

export function favoriteForPath(path: string): LocalFavorite {
  const segments = path.split("/").filter(Boolean);
  return { path, name: segments[segments.length - 1] ?? "/" };
}
