import { useEffect, type RefObject } from "react";
const focusable = "button, input, select, textarea, a[href], [tabindex]";
/** Keep keyboard navigation in the current modal and restore its invoking control. */
export function useModalFocus(key: unknown, preferred?: RefObject<HTMLElement | null>) {
  useEffect(() => {
    if (!key) return;
    const dialogs = document.querySelectorAll<HTMLElement>('[role="dialog"][aria-modal="true"]');
    const dialog = dialogs[dialogs.length - 1];
    if (!dialog) return;
    const previous = document.activeElement as HTMLElement | null;
    if (!dialog.hasAttribute("tabindex")) dialog.tabIndex = -1;
    const controls = () =>
      [...dialog.querySelectorAll<HTMLElement>(focusable)].filter(
        (element) =>
          element.tabIndex >= 0 &&
          !element.matches(":disabled") &&
          !element.closest('[hidden], [aria-hidden="true"]'),
      );
    const first = () =>
      preferred?.current && !preferred.current.matches(":disabled")
        ? preferred.current
        : (controls()[0] ?? dialog);
    if (!dialog.contains(document.activeElement)) first().focus();
    const onTab = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const items = controls();
      const index = items.indexOf(document.activeElement as HTMLElement);
      if (
        !items.length ||
        index < 0 ||
        (event.shiftKey ? index === 0 : index === items.length - 1)
      ) {
        event.preventDefault();
        (event.shiftKey ? (items[items.length - 1] ?? dialog) : (items[0] ?? dialog)).focus();
      }
    };
    const keepFocus = (event: FocusEvent) => {
      if (!dialog.contains(event.target as Node)) first().focus();
    };
    document.addEventListener("keydown", onTab, true);
    document.addEventListener("focusin", keepFocus);
    return () => {
      document.removeEventListener("keydown", onTab, true);
      document.removeEventListener("focusin", keepFocus);
      if (previous?.isConnected) previous.focus();
    };
  }, [key, preferred]);
}
