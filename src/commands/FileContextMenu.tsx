import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n/I18nProvider";
import {
  contextItems,
  presentNativeMenu,
  type CapturedMenu,
  type ContextCommand,
  type ContextMenuClient,
} from "./contextMenu";
export function FileContextMenu({
  menu,
  client,
  busy,
  showHidden,
  onChoose,
  onClose,
}: {
  menu: CapturedMenu;
  client: ContextMenuClient;
  busy: boolean;
  showHidden: boolean;
  onChoose: (command: ContextCommand) => void;
  onClose: () => void;
}) {
  const { language, t } = useI18n();
  const [captured] = useState(() => ({ menu, language, showHidden, busy }));
  const [fallback, setFallback] = useState(!client.available());
  const callback = useRef(onChoose),
    close = useRef(onClose),
    root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    callback.current = onChoose;
    close.current = onClose;
  });
  useEffect(() => {
    if (!client.available()) return;
    let disposed = false,
      stop: (() => void) | undefined;
    void presentNativeMenu(
      client,
      captured.menu,
      captured.language,
      captured.showHidden,
      captured.busy,
      (command) => {
        if (!disposed) callback.current(command);
      },
      () => {
        if (!disposed) setFallback(true);
      },
      () => !disposed,
    )
      .then((cleanup) => {
        if (disposed) cleanup();
        else stop = cleanup;
      })
      .catch(() => {
        if (!disposed) setFallback(true);
      });
    return () => {
      disposed = true;
      stop?.();
    };
    // Menu language, enabled state and selection are captured at opening time.
  }, [client, captured]);
  useEffect(() => {
    if (!fallback) return;
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    root.current?.focus();
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) close.current();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        close.current();
        previous?.focus();
      }
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape, true);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape, true);
    };
  }, [fallback]);
  if (!fallback) return null;
  const items = contextItems(captured.menu, captured.language, captured.busy);
  return (
    <div
      ref={root}
      className="context-menu"
      role="menu"
      tabIndex={-1}
      style={{
        left: Math.max(0, Math.min(menu.x, window.innerWidth - 272)),
        top: Math.max(0, Math.min(menu.y, window.innerHeight - (items.length * 28 + 48))),
      }}
      onKeyDown={(event) => {
        const command: ContextCommand | undefined = event.metaKey
          ? (
              {
                c: "copy",
                x: "cut",
                v: "paste",
                Backspace: "trash",
                ArrowDown: "open",
                r: "refresh",
              } as Record<string, ContextCommand>
            )[event.key]
          : (
              { F2: "rename", F5: "copyToOther", F6: "moveToOther" } as Record<
                string,
                ContextCommand
              >
            )[event.key];
        if (command) {
          event.preventDefault();
          if (items.some((item) => item.command === command && item.enabled))
            callback.current(command);
          return;
        }
        if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
        event.preventDefault();
        const buttons = Array.from(
          root.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [],
        );
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const next =
          index < 0
            ? event.key === "ArrowDown"
              ? 0
              : buttons.length - 1
            : (index + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
        buttons[next]?.focus();
      }}
    >
      {client.available() ? (
        <span className="context-menu-status" role="status">
          {t("menu.fallback")}
        </span>
      ) : null}
      {items.map((item, index) => (
        <div key={item.command}>
          {index > 0 && items[index - 1].group !== item.group ? (
            <div className="context-menu-separator" role="separator" />
          ) : null}
          <button
            role={item.command === "toggleHidden" ? "menuitemcheckbox" : "menuitem"}
            aria-checked={item.command === "toggleHidden" ? showHidden : undefined}
            disabled={!item.enabled}
            type="button"
            onClick={() => callback.current(item.command)}
          >
            <span>{item.label}</span>
            <span className="context-menu-shortcut" aria-hidden="true">
              {item.shortcut}
            </span>
          </button>
        </div>
      ))}
    </div>
  );
}
