import "./components.css";

/** A native progress bar with a text label (the numbers are always on screen too). */
export function ProgressBar({ value, max, label }: { value: number; max: number; label: string }) {
  return (
    <progress className="progress" value={value} max={Math.max(max, 1)} aria-label={label}>
      {value} of {max}
    </progress>
  );
}
