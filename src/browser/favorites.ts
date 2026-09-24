export const FAVORITES_STORAGE_KEY = "panedeck.local-favorites.v1";
const MAX_FAVORITES = 100;

export interface LocalFavorite {
  name: string;
  path: string;
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
