import { useCallback, useState } from "react";
import { isApiError } from "../api/errors";

export type ActionOutcome =
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "ok"; message: string }
  | { kind: "error"; message: string };

/**
 * Run one API action. On success the server's message is kept and `onDone` runs (the screen
 * re-reads); on refusal the API's own message is kept and nothing is re-read or changed.
 */
export function useAction(onDone: () => void) {
  const [outcome, setOutcome] = useState<ActionOutcome>({ kind: "idle" });
  const run = useCallback(
    async (call: () => Promise<{ message: string }>): Promise<boolean> => {
      setOutcome({ kind: "pending" });
      try {
        const r = await call();
        setOutcome({ kind: "ok", message: r.message });
        onDone();
        return true;
      } catch (e) {
        setOutcome({ kind: "error", message: isApiError(e) ? e.message : String(e) });
        return false;
      }
    },
    [onDone],
  );
  const clear = useCallback(() => setOutcome({ kind: "idle" }), []);
  return { outcome, run, clear, pending: outcome.kind === "pending" };
}
