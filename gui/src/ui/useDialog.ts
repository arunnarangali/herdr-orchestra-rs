import { useEffect, useRef, type KeyboardEvent } from "react";

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * What every overlay (dialog, drawer, palette) does for the keyboard: focus moves in when it
 * opens, Tab stays inside, Escape closes, and focus goes back to where it was. Built by hand
 * because `<dialog>` is not reliable in every webview.
 */
export function useDialog<E extends HTMLElement>(open: boolean, onClose: () => void, initialFocus?: string) {
  const ref = useRef<E>(null);

  useEffect(() => {
    if (!open) return;
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const box = ref.current;
    const target =
      (initialFocus ? box?.querySelector<HTMLElement>(initialFocus) : null) ??
      box?.querySelector<HTMLElement>(FOCUSABLE) ??
      box;
    target?.focus();
    return () => before?.focus();
  }, [open, initialFocus]);

  const onKeyDown = (e: KeyboardEvent<HTMLElement>) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key !== "Tab") return;
    const items = [...(ref.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])];
    const first = items[0];
    const last = items[items.length - 1];
    if (!first || !last) return e.preventDefault();
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };

  return { ref, onKeyDown };
}
