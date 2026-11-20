import type { ReactNode, TableHTMLAttributes } from "react";

export interface TableProps extends TableHTMLAttributes<HTMLTableElement> {
  /** Names the table for screen readers (shown to nobody else). */
  caption: string;
  children: ReactNode;
}

/** A data table that scrolls sideways on a small window instead of breaking the layout. */
export function Table({ caption, className, children, ...rest }: TableProps) {
  return (
    <div className="table-wrap">
      <table className={["table", className].filter(Boolean).join(" ")} {...rest}>
        <caption className="visually-hidden">{caption}</caption>
        {children}
      </table>
    </div>
  );
}
