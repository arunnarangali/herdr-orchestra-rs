import { useState } from "react";
import { ErrorPanel } from "../../ui/ResourceView";
import { useResource } from "../../ui/useResource";
import { AgentsTab } from "./AgentsTab";
import { ModelsTab } from "./ModelsTab";
import "./agents.css";

type Tab = "agents" | "models";

/** The agents installed on this machine, the run's team, and the model catalog with its policy. */
export function AgentsScreen() {
  const [tab, setTab] = useState<Tab>("agents");
  const agents = useResource((api, signal) => api.agents({ signal }), []);
  const models = useResource((api, signal) => api.models({ signal }), []);
  // These two only exist when there is a run; their absence is a normal state, not an error.
  const status = useResource((api, signal) => api.status({ signal }), []);
  const plan = useResource((api, signal) => api.plan({ signal }), []);
  const noRun = (r: { error: { isNoRun: boolean } | null }) => r.error?.isNoRun === true;

  const current = tab === "agents" ? agents : models;
  const tabs: { id: Tab; label: string }[] = [
    { id: "agents", label: "Agents" },
    { id: "models", label: "Models" },
  ];
  const onKey = (e: React.KeyboardEvent) => {
    if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
    const next: Tab = tab === "agents" ? "models" : "agents";
    setTab(next);
    document.getElementById(`ag-tab-${next}`)?.focus();
  };

  return (
    <section className="screen agents" aria-labelledby="screen-title">
      <h1 id="screen-title">Agents</h1>
      <div role="tablist" aria-label="Agents and models" className="ag-tabs" onKeyDown={onKey}>
        {tabs.map((t) => (
          <button
            key={t.id}
            id={`ag-tab-${t.id}`}
            type="button"
            role="tab"
            aria-selected={tab === t.id}
            aria-controls="ag-panel"
            tabIndex={tab === t.id ? 0 : -1}
            className={tab === t.id ? "ag-tab ag-tab--on" : "ag-tab"}
            onClick={() => setTab(t.id)}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div id="ag-panel" role="tabpanel" aria-labelledby={`ag-tab-${tab}`}>
        {current.state === "loading" && (
          <p role="status" className="muted">
            Loading…
          </p>
        )}
        {current.state === "error" && current.error && <ErrorPanel error={current.error} onRetry={current.reload} />}
        {tab === "agents" && agents.data && (
          <AgentsTab data={agents.data} status={noRun(status) ? null : status.data} />
        )}
        {tab === "models" && models.data && plan.state !== "loading" && (
          <ModelsTab models={models.data} plan={noRun(plan) ? null : plan.data} agents={agents.data} />
        )}
      </div>
    </section>
  );
}
