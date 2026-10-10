import { useEffect, useRef } from "react";
import type { OperationClient, OperationSnapshot } from "../browser/operationApi";
export function useOperationEvents(
  client: OperationClient,
  receive: (snapshot: OperationSnapshot) => void,
  failed: () => void,
) {
  const callbacks = useRef({ receive, failed });
  useEffect(() => {
    callbacks.current = { receive, failed };
  }, [receive, failed]);
  useEffect(() => {
    let disposed = false,
      generation = 0,
      stop: (() => void) | undefined;
    const connect = async () => {
      const current = ++generation;
      stop?.();
      stop = undefined;
      const accept = (snapshot: OperationSnapshot) => {
        if (!disposed && current === generation) callbacks.current.receive(snapshot);
      };
      try {
        const cleanup = client.watch
          ? await client.watch(accept)
          : await client.listJobs().then((jobs) => {
              accept({
                sequence: 0,
                jobs,
                logStatus: { persistenceEnabled: false, failedWrites: 0, lastError: null },
                recoveryLogStatus: { persistenceEnabled: false, failedWrites: 0, lastError: null },
              });
              return () => {};
            });
        if (disposed || current !== generation) cleanup();
        else stop = cleanup;
      } catch {
        if (!disposed && current === generation) callbacks.current.failed();
      }
    };
    void connect();
    window.addEventListener("focus", connect);
    return () => {
      disposed = true;
      generation++;
      stop?.();
      window.removeEventListener("focus", connect);
    };
  }, [client]);
}
