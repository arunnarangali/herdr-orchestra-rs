import type { ElementType, HTMLAttributes, ReactNode } from "react";
import type { Tone } from "./Button";

export interface CardProps extends HTMLAttributes<HTMLElement> {
  /** Tints the card and its border (an item that needs attention, a failure, …). */
  tone?: Tone;
  as?: ElementType;
}

/** The panel every screen is made of. */
export function Card({ tone, as: As = "div", className, children, ...rest }: CardProps) {
  const cls = ["card", tone && `card--tone tone-${tone}`, className].filter(Boolean).join(" ");
  return (
    <As className={cls} {...rest}>
      {children}
    </As>
  );
}

/** The header row of a card: icon and title on the left, actions on the right. */
export function CardHead({ title, icon, children, level = 2 }: { title: ReactNode; icon?: ReactNode; children?: ReactNode; level?: 2 | 3 }) {
  const H = level === 2 ? "h2" : "h3";
  return (
    <div className="card__head">
      <H className="card__title">
        {icon}
        {title}
      </H>
      {children}
    </div>
  );
}
