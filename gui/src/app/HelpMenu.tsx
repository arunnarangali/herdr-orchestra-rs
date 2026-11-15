import { useEffect, useId, useRef, useState } from "react";

export interface HelpMenuProps {
  onOnboarding: () => void;
  onShortcuts: () => void;
}

/** The Help menu in the top bar: Getting started and the shortcut list. */
export function HelpMenu({ onOnboarding, onShortcuts }: HelpMenuProps) {
  const [open, setOpen] = useState(false);
  const menuId = useId();
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    const onDown = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [open]);

  const close = (refocus: boolean) => {
    setOpen(false);
    if (refocus) button.current?.focus();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      close(true);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      const items = [...(root.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
      const at = items.indexOf(document.activeElement as HTMLElement);
      const next = (at + (e.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
      items[next]?.focus();
      e.preventDefault();
    } else if (e.key === "Tab") {
      setOpen(false);
    }
  };

  const pick = (action: () => void) => () => {
    close(false);
    action();
  };

  return (
    <div className="menu" ref={root} onKeyDown={onKeyDown}>
      <button
        ref={button}
        type="button"
        className="button"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen((o) => !o)}
      >
        Help
      </button>
      {open && (
        <div id={menuId} role="menu" aria-label="Help" className="menu-list">
          <button type="button" role="menuitem" className="menu-item" onClick={pick(onOnboarding)}>
            Getting started
          </button>
          <button type="button" role="menuitem" className="menu-item" onClick={pick(onShortcuts)}>
            Keyboard shortcuts <kbd>?</kbd>
          </button>
        </div>
      )}
    </div>
  );
}
