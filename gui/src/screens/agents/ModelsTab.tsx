import type { AgentsResponse, ModelInfo, ModelsResponse, PlanResponse } from "../../api/types";
import { tierRank } from "./advice";

const RATINGS: { key: keyof ModelInfo; label: string }[] = [
  { key: "reasoning", label: "Reasoning" },
  { key: "coding", label: "Coding" },
  { key: "architecture", label: "Architecture" },
  { key: "instructions", label: "Instructions" },
  { key: "tools", label: "Tools" },
  { key: "planning", label: "Planning" },
  { key: "speed", label: "Speed" },
];

/** Ratings run from 1 to 5. Each is a labelled bar with its number beside it. */
function Rating({ model, k, label }: { model: ModelInfo; k: keyof ModelInfo; label: string }) {
  const v = model[k];
  return (
    <div className="ag-rating">
      <span className="ag-rating-label">{label}</span>
      {typeof v === "number" ? (
        <>
          <meter min={0} max={5} value={v} aria-label={`${label} of ${model.id}`} />
          <span>{v}</span>
        </>
      ) : (
        <span className="muted">not rated</span>
      )}
    </div>
  );
}

export interface ModelsTabProps {
  models: ModelsResponse;
  plan: PlanResponse | null;
  agents: AgentsResponse | null;
}

export function ModelsTab({ models, plan, agents }: ModelsTabProps) {
  const denied = models.models.filter((m) => m.denied);
  const team = agents?.team ?? [];
  return (
    <>
      <section className="panel" aria-label="Policy">
        <h2 className="panel-title">Kept out of runs ({denied.length})</h2>
        <p className="muted">
          {models.policy.deny.length > 0
            ? `Policy denies: ${models.policy.deny.join(", ")}${models.policy.deny_why ? `. ${models.policy.deny_why}` : "."}`
            : "The policy denies no model."}
        </p>
        {denied.length === 0 ? (
          <p className="muted">No model in the catalog is denied.</p>
        ) : (
          <ul>
            {denied.map((m) => (
              <li key={m.id}>
                <strong>{m.id}</strong> <span className="badge badge--danger">denied</span>{" "}
                <span className="muted">by {m.denied_by}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="panel" aria-label="Tiers">
        <h2 className="panel-title">Tiers</h2>
        <ul>
          {models.tiers.map((t) => (
            <li key={t.name}>
              <strong>{t.name}</strong> <span className="muted">needs reasoning {t.reasoning_floor}+</span>
              {t.purpose && <span>: {t.purpose}</span>}
            </li>
          ))}
        </ul>
      </section>

      <section className="panel" aria-label="Plan tiers against the team">
        <h2 className="panel-title">What the plan asks for and who can serve it</h2>
        {!plan ? (
          <p className="muted">There is no run in this project yet, so there is no plan to compare with the team.</p>
        ) : plan.milestones.length === 0 ? (
          <p className="muted">The plan has no milestones.</p>
        ) : (
          <div className="ag-wrap">
            <table className="ag-table">
              <caption className="visually-hidden">Milestone tiers against the team</caption>
              <thead>
                <tr>
                  <th scope="col">Milestone</th>
                  <th scope="col">Tier asked</th>
                  <th scope="col">Team members that can serve it</th>
                </tr>
              </thead>
              <tbody>
                {plan.milestones.map((m) => {
                  const need = tierRank(m.tier);
                  const can = team.filter((t) => t.role === "worker" && tierRank(t.tier) >= need);
                  return (
                    <tr key={m.id}>
                      <th scope="row">
                        {m.id} {m.title}
                      </th>
                      <td>{m.tier ?? "any"}</td>
                      <td>
                        {can.length > 0 ? (
                          can.map((t) => `${t.name} (${t.tier ?? "unrated"})`).join(", ")
                        ) : (
                          <span className="badge badge--danger">nobody on the team can serve this tier</span>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </section>

      <section aria-label="Catalog">
        <h2>Catalog ({models.models.length})</h2>
        {models.models.length === 0 && <p className="muted">The catalog is empty.</p>}
        <ul className="ag-models">
          {models.models.map((m) => (
            <li key={m.id} className="panel ag-model">
              <h3 className="panel-title">
                {m.id} {m.tier ? <span className="badge badge--info">{m.tier}</span> : <span className="badge badge--muted">unrated</span>}{" "}
                {m.denied && <span className="badge badge--danger">denied</span>}
              </h3>
              <p className="muted">
                Runs on: {m.kinds.join(", ") || "–"} · open weights:{" "}
                {m.open_weights === null ? "unknown" : m.open_weights ? "yes" : "no"} · context:{" "}
                {m.context_k === null ? "unknown" : `${m.context_k}k`}
              </p>
              <div className="ag-ratings">
                {RATINGS.map((r) => (
                  <Rating key={r.key} model={m} k={r.key} label={r.label} />
                ))}
              </div>
              {m.evidence && <p className="muted">Source: {m.evidence}</p>}
            </li>
          ))}
        </ul>
      </section>
    </>
  );
}
