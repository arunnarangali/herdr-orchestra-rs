import { useId, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Icon, type IconName } from "./Icon";
import { useDialog } from "./useDialog";

export interface DrawerProps {
  open: boolean;
  title: string;
  icon?: IconName;
  onClose: () => void;
  /** A row pinned under the content (a link to the full screen). */
  footer?: ReactNode;
  children: ReactNode;
}

/** A panel that slides in from the right. Same keyboard behaviour as `Modal`. */
export function Drawer({ open, title, icon, onClose, footer, children }: DrawerProps) {
  const { ref, onKeyDown } = useDialog<HTMLElement>(open, onClose);
  const titleId = useId();
  if (!open) return null;
  return createPortal(
    <div className="drawer-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <aside ref={ref} className="drawer" role="dialog" aria-modal="true" aria-labelledby={titleId} tabIndex={-1} onKeyDown={onKeyDown}>
        <div className="drawer__head">
          <h2 className="drawer__title" id={titleId}>
            {icon && <Icon name={icon} />}
            {title}
          </h2>
          <button type="button" className="btn btn--ghost btn--icon btn--sm" aria-label={`Close ${title.toLowerCase()}`} onClick={onClose}>
            <Icon name="x" />
          </button>
        </div>
        <div className="drawer__body">{children}</div>
        {footer && <div className="drawer__foot">{footer}</div>}
      </aside>
    </div>,
    document.body,
  );
}
