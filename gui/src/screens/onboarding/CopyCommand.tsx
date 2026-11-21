import { useState } from "react";

/** A command shown as text with a Copy button. The app only shows it; the user runs it. */
export function CopyCommand({ command }: { command: string }) {
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setState("copied");
    } catch {
      setState("failed");
    }
  };
  return (
    <div className="ob-command">
      <code>{command}</code>
      <button type="button" className="btn btn--sm" onClick={() => void copy()} aria-label={`Copy ${command}`}>
        {state === "copied" ? "Copied" : "Copy"}
      </button>
      {state === "failed" && <span className="error-text"> Could not copy: select the text and copy it.</span>}
    </div>
  );
}
