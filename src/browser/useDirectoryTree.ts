import { useEffect, useMemo, useSyncExternalStore } from "react";
import type { BrowseOptions, DirectoryClient, DirectoryListing } from "./directoryApi";
import { DirectoryTree } from "./directoryTree";
export function useDirectoryTree(
  client: DirectoryClient,
  root: DirectoryListing | null,
  options: BrowseOptions,
) {
  const path = root?.treeKey ?? root?.displayPath;
  const pane = options.pane;
  // A distinct model per pane/root; updates within the same root retain expansions.
  const tree = useMemo(
    () =>
      new DirectoryTree(
        client,
        null,
        {
          pane,
          showHidden: false,
          sortField: "name",
          sortDirection: "ascending",
        },
        path,
      ),
    [client, pane, path],
  );
  const viewOptions = useMemo(
    () => ({
      pane,
      showHidden: options.showHidden,
      sortField: options.sortField,
      sortDirection: options.sortDirection,
    }),
    [pane, options.showHidden, options.sortField, options.sortDirection],
  );
  useEffect(() => {
    tree.start();
    return () => tree.dispose();
  }, [tree]);
  useEffect(() => {
    tree.update(root, viewOptions);
  }, [tree, root, viewOptions]);
  const rows = useSyncExternalStore(tree.subscribe, tree.snapshot);
  return { tree, rows };
}
