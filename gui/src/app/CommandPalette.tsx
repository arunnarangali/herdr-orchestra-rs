import { useEffect, useId, useMemo, useState, type KeyboardEvent } from "react";
import { Icon, type IconName } from "../ui/Icon";
import { Modal } from "../ui/Modal";

export interface Command {
  /** Section heading: "Go to", "Projects", "Actions". */
  group: string;
  label: string;
  icon: IconName;
  /** A short right-aligned hint (the shortcut, the branch). */
  hint?: string;
  run: () => void;
}

export interface CommandPaletteProps {
  open: boolean;
  commands: readonly Command[];
  onClose: () => void;
}

/**
 * Ctrl+K. Lists every command, filters as you type, Up/Down move, Enter runs the highlighted one
 * and Escape closes. The dialog traps focus and gives it back (see `ui/useDialog`).
 */
export function CommandPalette({ open, commands, onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const listId = useId();

  useEffect(() => {
    if (open) {
      setQuery("");
      setActive(0);
    }
  }, [open]);

  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    return commands.filter((c) => !q || `${c.group} ${c.label}`.toLowerCase().includes(q));
  }, [commands, query]);

  const at = shown.length === 0 ? 0 : Math.min(active, shown.length - 1);
  const run = (c: Command) => {
    onClose();
    c.run();
  };

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (shown.length) setActive((at + (e.key === "ArrowDown" ? 1 : -1) + shown.length) % shown.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const c = shown[at];
      if (c) run(c);
    }
  };

  let group = "";
  return (
    <Modal open={open} title="Command palette" hideTitle top className="palette" onClose={onClose} initialFocus="input">
      <div className="palette__search">
        <Icon name="search" />
        <input
          className="palette__input"
          type="text"
          placeholder="Type a command or page…"
          autoComplete="off"
          spellCheck={false}
          role="combobox"
          aria-expanded="true"
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={shown.length ? `${listId}-${at}` : undefined}
          aria-label="Command palette"
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={onKeyDown}
        />
      </div>
      <div className="palette__list" id={listId} role="listbox" aria-label="Commands">
        {shown.length === 0 && <div className="palette__empty">No matching commands</div>}
        {shown.map((c, i) => {
          const heading = c.group !== group ? ((group = c.group), <div className="palette__group" role="presentation">{c.group}</div>) : null;
          return (
            <div key={`${c.group}/${c.label}`} role="presentation">
              {heading}
              <div
                id={`${listId}-${i}`}
                className="palette__item"
                role="option"
                aria-selected={i === at}
                onClick={() => run(c)}
                onPointerMove={() => i !== at && setActive(i)}
              >
                <Icon name={c.icon} />
                <span>{c.label}</span>
                {c.hint && <span className="palette__hint">{c.hint}</span>}
              </div>
            </div>
          );
        })}
      </div>
      <div className="palette__foot">
        <span>
          <kbd>↑</kbd> <kbd>↓</kbd> navigate
        </span>
        <span>
          <kbd>Enter</kbd> run
        </span>
        <span>
          <kbd>Esc</kbd> close
        </span>
      </div>
    </Modal>
  );
}
