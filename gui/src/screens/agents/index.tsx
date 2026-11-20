import { useState, type KeyboardEvent } from "react";
import { Button } from "../../ui/Button";
import { Icon, type IconName } from "../../ui/Icon";
import { ErrorPanel } from "../../ui/ResourceView";
import { useResource } from "../../ui/useResource";
import { AgentOutputModal } from "./AgentOutputModal";
import { AgentsTab } from "./AgentsTab";
import { Catalog, PolicyAndTiers } from "./ModelsTab";
import { NOT_AVAILABLE } from "./notAvailable";
import { TeamTab } from "./TeamTab";
import "./agents.css";

type Tab = "installed" | "catalog" | "team";
const ORDER: Tab[] = ["installed", "catalog", "team"];

/** The agents installed on this machine, the model catalog with its policy, and the run's team. */
export function AgentsScreen() {
  const [tab, setTab] = useState<Tab>("installed");
  const [outputAgent, setOutputAgent] = useState<string | null>(null);
  const agents = useResource((api, signal) => api.agents({ signal }), [], {
    refreshOn: ["task-state", "lead-event", "run-state", "attention"],
  });
  const models = useResource((api, signal) => api.models({ signal }), []);
  // These two only exist when there is a run; their absence is a normal state, not an error.
  const status = useResource((api, signal) => api.status({ signal }), [], {
    refreshOn: ["task-state", "run-state"],
  });
  const plan = useResource((api, signal) => api.plan({ signal }), []);
  const noRun = (r: { error: { isNoRun: boolean } | null }) => r.error?.isNoRun === true;

  const scanning = agents.refreshing || models.refreshing;
  const scan = () => {
    agents.reload();
    models.reload();
  };

  const tabs: { id: Tab; icon: IconName; label: string }[] = [
    { id: "installed", icon: "cube", label: `Installed Agents${agents.data ? ` (${agents.data.agents.length})` : ""}` },
    { id: "catalog", icon: "grid", label: `Model Catalog & Policy${models.data ? ` (${models.data.models.length})` : ""}` },
    { id: "team", icon: "robot", label: "Team Builder" },
  ];
  const go = (t: Tab) => {
    setTab(t);
    document.getElementById(`ag-tab-${t}`)?.focus();
  };
  const onKey = (e: KeyboardEvent) => {
    const i = ORDER.indexOf(tab);
    const next =
      e.key === "ArrowRight" ? ORDER[(i + 1) % ORDER.length] : e.key === "ArrowLeft" ? ORDER[(i - 1 + ORDER.length) % ORDER.length] : e.key === "Home" ? ORDER[0] : e.key === "End" ? ORDER[ORDER.length - 1] : null;
    if (!next) return;
    e.preventDefault();
    go(next);
  };

  // What this tab needs before it can show anything.
  const needed = tab === "catalog" ? [models] : tab === "installed" ? [agents, models] : [agents];
  const loading = needed.some((r) => r.state === "loading");
  const failed = needed.find((r) => r.state === "error" && r.error);

  return (
    <section className="screen ag" aria-labelledby="screen-title">
      <header className="pg-head">
        <span className="pg-head__icon">
          <Icon name="robot" />
        </span>
        <div className="pg-head__titles">
          <h1 id="screen-title" className="pg-head__title">
            Agents &amp; Models
          </h1>
          <p className="pg-head__sub">Discover, manage and assign agent CLIs, models and role policies for orchestration.</p>
        </div>
      </header>

      <div className="bar2">
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
              className="ag-tab"
              onClick={() => setTab(t.id)}
            >
              <Icon name={t.icon} />
              {t.label}
            </button>
          ))}
        </div>
        <div className="bar2__actions">
          <Button onClick={scan} disabled={scanning} aria-busy={scanning}>
            <Icon name="sync" />
            {scanning ? "Scanning…" : "Scan Agents"}
          </Button>
          <Button unavailable={`${NOT_AVAILABLE}: run \`orch agents sync\``}>
            <Icon name="sync" />
            Sync Skills (orch agents sync)
          </Button>
          <Button unavailable={`${NOT_AVAILABLE}: run \`orch doctor\``}>
            <Icon name="help" />
            Run Doctor
          </Button>
        </div>
      </div>

      <div id="ag-panel" role="tabpanel" aria-labelledby={`ag-tab-${tab}`} className="ag-panel">
        {loading && (
          <p role="status" className="muted">
            Loading…
          </p>
        )}
        {!loading && failed?.error && <ErrorPanel error={failed.error} onRetry={failed.reload} />}
        {!loading && !failed && tab === "installed" && agents.data && (
          <>
            <AgentsTab data={agents.data} models={models.data} onRecheck={scan} />
            {models.data && <Catalog models={models.data} withTools={false} />}
          </>
        )}
        {!loading && !failed && tab === "catalog" && models.data && (
          <>
            <Catalog models={models.data} withTools />
            <PolicyAndTiers models={models.data} />
          </>
        )}
        {!loading && !failed && tab === "team" && agents.data && plan.state !== "loading" && (
          <TeamTab
            data={agents.data}
            status={noRun(status) ? null : status.data}
            plan={noRun(plan) ? null : plan.data}
            onOpenOutput={setOutputAgent}
          />
        )}
      </div>

      {outputAgent && <AgentOutputModal agentName={outputAgent} onClose={() => setOutputAgent(null)} />}
    </section>
  );
}
