import type { ButtonHTMLAttributes } from "react";

export type Tone = "emerald" | "cyan" | "amber" | "coral" | "violet" | "neutral";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  /** Colour of the button; none is the plain outlined button. */
  tone?: Tone;
  /** A filled button (the main action). Needs a tone. */
  solid?: boolean;
  ghost?: boolean;
  size?: "md" | "sm";
  block?: boolean;
  /** A square button holding only an icon (give it an `aria-label`). */
  icon?: boolean;
  /**
   * The action the API does not offer yet: the button is shown disabled with this as its
   * reason (also as its tooltip), never a pretend success.
   */
  unavailable?: string;
}

/** Class names of a button, for a link or other element that must look like one. */
export function buttonClass({ tone, solid, ghost, size, block, icon }: Pick<ButtonProps, "tone" | "solid" | "ghost" | "size" | "block" | "icon">, extra?: string): string {
  return [
    "btn",
    tone && tone !== "neutral" && `btn--${tone}`,
    solid && "btn--solid",
    ghost && "btn--ghost",
    size === "sm" && "btn--sm",
    block && "btn--block",
    icon && "btn--icon",
    extra,
  ]
    .filter(Boolean)
    .join(" ");
}

export function Button({ tone, solid, ghost, size, block, icon, unavailable, className, type = "button", disabled, title, children, ...rest }: ButtonProps) {
  return (
    <button
      type={type}
      className={buttonClass({ tone, solid, ghost, size, block, icon }, className)}
      disabled={disabled || unavailable !== undefined}
      title={unavailable ?? title}
      {...rest}
    >
      {children}
      {unavailable !== undefined && <span className="visually-hidden"> ({unavailable})</span>}
    </button>
  );
}
