import type { ReactNode } from "react";

export interface SegmentedOption<V extends string> {
  value: V;
  label: ReactNode;
  /** Plain-text name when `label` is not text. */
  ariaLabel?: string;
}

export interface SegmentedProps<V extends string> {
  /** Names the group. */
  label: string;
  options: readonly SegmentedOption<V>[];
  value: V;
  onChange: (value: V) => void;
}

/** A row of exclusive choices (two views of the same data). */
export function Segmented<V extends string>({ label, options, value, onChange }: SegmentedProps<V>) {
  return (
    <div className="seg" role="group" aria-label={label}>
      {options.map((o) => (
        <button key={o.value} type="button" aria-pressed={o.value === value} aria-label={o.ariaLabel} onClick={() => onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}
