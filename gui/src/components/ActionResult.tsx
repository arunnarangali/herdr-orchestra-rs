import type { ActionOutcome } from "./useAction";
import "./components.css";

/** What the last action did: the server's message, or the API's refusal. */
export function ActionResult({ outcome }: { outcome: ActionOutcome }) {
  if (outcome.kind === "ok") {
    return (
      <p className="action-result action-result--ok" role="status">
        {outcome.message}
      </p>
    );
  }
  if (outcome.kind === "error") {
    return (
      <p className="action-result action-result--error" role="alert">
        {outcome.message}
      </p>
    );
  }
  return null;
}
