import { useEffect, useRef } from "react";
import { screens } from "./routes";
import { Modal } from "../ui/Modal";

/** How long after `g` the second key still counts. */
const CHORD_MS = 1500;

function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return (
    target.isContentEditable ||
    ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)
  );
}

export interface ShortcutHandlers {
  /** `g` then the screen's key. */
  goTo: (path: string) => void;
  /** `?` */
  showHelp: () => void;
}

/**
 * Keyboard navigation: `g` then a letter opens a screen (`g o` Overview, `g p` Projects, …)
 * and `?` lists them. Ignored while typing in a field or with Ctrl, Alt or Meta held, so it
 * never fights the browser or the text being entered.
 */
export function useShortcuts({ goTo, showHelp }: ShortcutHandlers): void {
  const handlers = useRef({ goTo, showHelp });
  handlers.current = { goTo, showHelp };

  useEffect(() => {
    let armed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const disarm = () => {
      armed = false;
      clearTimeout(timer);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.ctrlKey || e.metaKey || e.altKey || isTyping(e.target)) return;
      if (e.key === "Escape") return disarm();
      if (armed) {
        disarm();
        const screen = screens.find((s) => s.shortcut === e.key.toLowerCase());
        if (screen) {
          e.preventDefault();
          handlers.current.goTo(screen.path);
        }
        return;
      }
      if (e.key === "g") {
        armed = true;
        timer = setTimeout(disarm, CHORD_MS);
      } else if (e.key === "?") {
        e.preventDefault();
        handlers.current.showHelp();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      clearTimeout(timer);
    };
  }, []);
}

/** The list of shortcuts, built from the screen registry. */
export function ShortcutList({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Modal open={open} title="Keyboard shortcuts" onClose={onClose}>
      <table className="shortcuts">
        <caption className="visually-hidden">Keyboard shortcuts</caption>
        <thead>
          <tr>
            <th scope="col">Keys</th>
            <th scope="col">Action</th>
          </tr>
        </thead>
        <tbody>
          {screens
            .filter((s) => s.shortcut)
            .map((s) => (
              <tr key={s.id}>
                <td>
                  <kbd>g</kbd> then <kbd>{s.shortcut}</kbd>
                </td>
                <td>Go to {s.label}</td>
              </tr>
            ))}
          <tr>
            <td>
              <kbd>Ctrl</kbd> <kbd>K</kbd>
            </td>
            <td>Open the command palette</td>
          </tr>
          <tr>
            <td>
              <kbd>?</kbd>
            </td>
            <td>Show this list</td>
          </tr>
          <tr>
            <td>
              <kbd>Esc</kbd>
            </td>
            <td>Close a dialog or menu</td>
          </tr>
        </tbody>
      </table>
      <div className="modal-actions">
        <button type="button" className="btn" onClick={onClose}>
          Close
        </button>
      </div>
    </Modal>
  );
}
