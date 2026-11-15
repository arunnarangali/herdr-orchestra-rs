import type { ReactNode } from "react";
import { Modal } from "./Modal";

export interface ConfirmDialogProps {
  open: boolean;
  title: string;
  /** What will happen, in plain words. */
  children: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  /** Styles the confirm button as dangerous (stop a run, discard work). */
  destructive?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * Asks before a destructive action. Focus starts on Cancel, so pressing Enter by reflex
 * does not confirm; Escape cancels.
 */
export function ConfirmDialog({
  open,
  title,
  children,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  destructive = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  return (
    <Modal open={open} title={title} onClose={onCancel} role="alertdialog" initialFocus="[data-cancel]">
      <div className="modal-body">{children}</div>
      <div className="modal-actions">
        <button type="button" className="button" data-cancel onClick={onCancel}>
          {cancelLabel}
        </button>
        <button
          type="button"
          className={destructive ? "button button--danger" : "button button--primary"}
          onClick={onConfirm}
        >
          {confirmLabel}
        </button>
      </div>
    </Modal>
  );
}
