import { useId, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useDialog } from "./useDialog";

export interface ModalProps {
  open: boolean;
  title: string;
  onClose: () => void;
  /** `alertdialog` for questions that need an answer. */
  role?: "dialog" | "alertdialog";
  /** Selector of the element to focus first; default the first focusable one. */
  initialFocus?: string;
  /** Names the dialog for screen readers without showing the title (the palette). */
  hideTitle?: boolean;
  /** Put the dialog near the top instead of the middle (the palette). */
  top?: boolean;
  className?: string;
  children: ReactNode;
}

/** A modal dialog: focus moves in, Tab stays inside, Escape closes, focus goes back. */
export function Modal({ open, title, onClose, role = "dialog", initialFocus, hideTitle, top, className, children }: ModalProps) {
  const { ref, onKeyDown } = useDialog<HTMLDivElement>(open, onClose, initialFocus);
  const titleId = useId();
  if (!open) return null;

  return createPortal(
    <div className={top ? "modal-backdrop modal-backdrop--top" : "modal-backdrop"} onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        ref={ref}
        className={["modal", className].filter(Boolean).join(" ")}
        role={role}
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onKeyDown={onKeyDown}
      >
        <h2 id={titleId} className={hideTitle ? "visually-hidden" : "modal-title"}>
          {title}
        </h2>
        {children}
      </div>
    </div>,
    document.body,
  );
}
