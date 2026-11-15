import { ErrorPanel } from "../../ui/ResourceView";
import { useResource } from "../../ui/useResource";
import { AppSettingsForm } from "./AppSettingsForm";
import { RunSettings } from "./RunSettings";
import "./settings.css";

/** The run's settings (read-only, from plan.md) and the app's own settings (saved by the shell). */
export function SettingsScreen() {
  const plan = useResource((api, signal) => api.plan({ signal }), []);
  const status = useResource((api, signal) => api.status({ signal }), []);
  const noRun = plan.error?.isNoRun === true;

  return (
    <section className="screen settings" aria-labelledby="screen-title">
      <h1 id="screen-title">Settings</h1>
      {plan.state === "loading" && (
        <p role="status" className="muted">
          Loading…
        </p>
      )}
      {noRun && (
        <div className="panel" role="status">
          <p className="panel-title">No run in this project yet</p>
          <p className="muted">The run's settings appear here once `.orchestrate/plan.md` exists and you have run `orch sync`.</p>
        </div>
      )}
      {plan.state === "error" && plan.error && !noRun && <ErrorPanel error={plan.error} onRetry={plan.reload} />}
      {plan.data && <RunSettings header={plan.data.header} status={status.data} />}
      <AppSettingsForm />
    </section>
  );
}
