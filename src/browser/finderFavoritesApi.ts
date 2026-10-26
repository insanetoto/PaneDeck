import { invoke, isTauri } from "@tauri-apps/api/core";

export interface FinderFavorite {
  id: string;
  name: string;
  path: string;
  available: boolean;
}

export interface FinderFavorites {
  favorites: FinderFavorite[];
  unresolved: number;
  failed: boolean;
  timedOut: boolean;
  visibilityAvailable?: boolean;
}

export interface FinderFavoritesClient {
  list(refresh?: boolean): Promise<FinderFavorites>;
}

const empty: FinderFavorites = {
  favorites: [],
  unresolved: 0,
  failed: false,
  timedOut: false,
  visibilityAvailable: true,
};

export const tauriFinderFavoritesClient: FinderFavoritesClient = {
  list: async (refresh = false) => {
    // The deprecated shared-list API only exposes the user list to a bundled
    // app; the browser preview has no Finder session to read.
    if (!isTauri()) return empty;
    return invoke("list_finder_favorites", { refresh });
  },
};
