import type { ReactNode } from "react";
import type { Tone } from "./Button";

export interface BadgeProps {
  tone?: Tone;
  mono?: boolean;
  pill?: boolean;
  children: ReactNode;
  className?: string;
}

/** A small label. The words always say it; the colour only adds to them. */
export function Badge({ tone = "neutral", mono, pill, className, children }: BadgeProps) {
  const cls = ["badge", `badge--${tone}`, mono && "badge--mono", pill && "badge--pill", className].filter(Boolean).join(" ");
  return <span className={cls}>{children}</span>;
}

/** A status dot. Decorative: put the state in words next to it. */
export function Dot({ tone = "neutral", pulse }: { tone?: Tone | "idle"; pulse?: boolean }) {
  return <span className={["dot", `dot--${tone}`, pulse && "dot--pulse"].filter(Boolean).join(" ")} aria-hidden="true" />;
}
