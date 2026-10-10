import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef, useState } from "react";
import type { Language } from "../i18n/I18nProvider";
import type { CommandContext } from "./context";
import type { FileCommand } from "./dispatch";

export type DesktopCommand =
  | FileCommand
  | "operationLog"
  | "refresh"
  | "toggleHidden"
  | "diagnostics"
  | "languageEnglish"
  | "languageChinese";
export interface DesktopMenuPresentation {
  language: Language;
  enabledCommands: DesktopCommand[];
  textEditing: boolean;
  showHidden: boolean;
}
export interface DesktopMenuSnapshot extends DesktopMenuPresentation {
  revision: string;
}
export interface DesktopMenuEvent {
  command: DesktopCommand;
  revision: string;
  eventId: string;
}
export interface DesktopMenuClient {
  available(): boolean;
  sync(snapshot: DesktopMenuSnapshot): Promise<void>;
  subscribe(handler: (event: DesktopMenuEvent) => void): Promise<() => void>;
}
export const tauriDesktopMenuClient: DesktopMenuClient = {
  available: isTauri,
  sync: (snapshot) => invoke("sync_desktop_menu", { snapshot }),
  subscribe: (handler) =>
    listen<DesktopMenuEvent>("desktop-menu-command", (event) => handler(event.payload)),
};

let revisionCounter = 0n;
function nextRevision() {
  const timestamp = BigInt(Date.now()) * 1_000_000n;
  revisionCounter = timestamp > revisionCounter ? timestamp : revisionCounter + 1n;
  return revisionCounter.toString();
}

/** A single subscription, serialized presentation updates, and revision-bound actions. */
export function useDesktopMenu(
  client: DesktopMenuClient,
  presentation: DesktopMenuPresentation,
  context: CommandContext,
  run: (command: DesktopCommand, context: CommandContext) => void,
) {
  const [ready, setReady] = useState(false);
  const [active, setActive] = useState(false);
  const [failed, setFailed] = useState(false);
  const latest = useRef<{ snapshot: DesktopMenuSnapshot; context: CommandContext } | null>(null);
  const callback = useRef(run);
  const lastEvent = useRef(-1n);
  const updates = useRef(Promise.resolve());
  useEffect(() => {
    callback.current = run;
  }, [run]);

  useEffect(() => {
    if (!client.available()) return;
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    void client
      .subscribe((event) => {
        const current = latest.current;
        if (
          disposed ||
          !current ||
          event.revision !== current.snapshot.revision ||
          !current.snapshot.enabledCommands.includes(event.command) ||
          !/^\d+$/.test(event.eventId)
        )
          return;
        const eventId = BigInt(event.eventId);
        if (eventId <= lastEvent.current) return;
        lastEvent.current = eventId;
        callback.current(event.command, current.context);
      })
      .then((cleanup) => {
        if (disposed) cleanup();
        else {
          unsubscribe = cleanup;
          setReady(true);
        }
      })
      .catch(() => {
        if (!disposed) setFailed(true);
      });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [client]);

  useEffect(() => {
    if (!ready) return;
    let disposed = false;
    const snapshot = { ...presentation, revision: nextRevision() };
    latest.current = { snapshot, context };
    updates.current = updates.current.then(async () => {
      if (disposed) return;
      try {
        await client.sync(snapshot);
        if (!disposed) {
          setActive(true);
          setFailed(false);
        }
      } catch {
        if (!disposed) {
          setActive(false);
          setFailed(true);
        }
      }
    });
    return () => {
      disposed = true;
    };
  }, [client, ready, presentation, context]);
  return { active, failed };
}
