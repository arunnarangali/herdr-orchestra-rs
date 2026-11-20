import { Fragment, useId, useState } from "react";
import type { ModelInfo, ModelsResponse } from "../../api/types";
import { Badge } from "../../ui/Badge";
import { Button, type Tone } from "../../ui/Button";
import { Card, CardHead } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { Table } from "../../ui/Table";
import { NOT_AVAILABLE } from "./notAvailable";

const RATINGS: { key: keyof ModelInfo; label: string }[] = [
  { key: "reasoning", label: "Reasoning" },
  { key: "coding", label: "Coding" },
  { key: "architecture", label: "Architecture" },
  { key: "instructions", label: "Instructions" },
  { key: "tools", label: "Tools" },
  { key: "planning", label: "Planning" },
  { key: "speed", label: "Speed" },
];
const TIER_TONE: Record<string, Tone> = { small: "neutral", medium: "amber", high: "coral" };

export type TierFilter = "all" | "small" | "medium" | "high" | "unrated";
export type PolicyFilter = "all" | "allowed" | "denied";
export interface ModelFilters {
  q: string;
  tier: TierFilter;
  policy: PolicyFilter;
}
export const NO_FILTER: ModelFilters = { q: "", tier: "all", policy: "all" };

/** The models that pass the filters: text in the id or the agent kinds, the tier, and the policy. */
export function filterModels(models: ModelInfo[], f: ModelFilters): ModelInfo[] {
  const q = f.q.trim().toLowerCase();
  return models.filter(
    (m) =>
      (!q || m.id.toLowerCase().includes(q) || m.kinds.some((k) => k.toLowerCase().includes(q))) &&
      (f.tier === "all" || (f.tier === "unrated" ? m.tier === null : m.tier === f.tier)) &&
      (f.policy === "all" || (f.policy === "denied") === m.denied),
  );
}

function Dots({ value, label }: { value: number | null; label: string }) {
  if (value === null) return <span className="muted">–</span>;
  return (
    <span className="score" title={`${label} ${value} of 5`}>
      <span className="score__k" aria-hidden="true">
        {label[0]}
      </span>
      <span className="mono">{value}/5</span>
      <span className="sdots" aria-hidden="true">
        {[1, 2, 3, 4, 5].map((i) => (
          <i key={i} className={i <= value ? "on" : undefined} />
        ))}
      </span>
    </span>
  );
}

function Ratings({ model }: { model: ModelInfo }) {
  return (
    <div className="ag-ratings">
      {RATINGS.map(({ key, label }) => {
        const v = model[key];
        return (
          <div className="ag-rating" key={key}>
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
      })}
      {model.evidence && <p className="muted ag-small">Source: {model.evidence}</p>}
    </div>
  );
}

function provider(m: ModelInfo): string {
  const kinds = m.kinds.join(", ") || "–";
  return m.open_weights === null ? kinds : `${kinds} · ${m.open_weights ? "Open-Weight" : "Proprietary"}`;
}

/** The shipped catalog: tier, kinds, ratings, context and whether the policy keeps it out. */
export function Catalog({ models, withTools }: { models: ModelsResponse; withTools: boolean }) {
  const [filters, setFilters] = useState<ModelFilters>(NO_FILTER);
  const [expanded, setExpanded] = useState<string | null>(null);
  const ids = { q: useId(), tier: useId(), policy: useId() };
  const rows = withTools ? filterModels(models.models, filters) : models.models;
  const set = (o: Partial<ModelFilters>) => setFilters((f) => ({ ...f, ...o }));

  return (
    <Card as="section" className="sect" aria-labelledby="ag-h-catalog">
      <CardHead title={<span id="ag-h-catalog">Shipped Model Catalog &amp; Deny Policy</span>} icon={<Icon name="grid" />}>
        <span className="sect__note">
          Models: {models.models.length} • Policy: allow / deny (pattern match)
        </span>
      </CardHead>
      {withTools && (
        <div className="tools">
          <div className="field tools__q">
            <label htmlFor={ids.q}>Search models</label>
            <input id={ids.q} type="search" value={filters.q} placeholder="Model id or agent kind" onChange={(e) => set({ q: e.target.value })} />
          </div>
          <div className="field">
            <label htmlFor={ids.tier}>Tier</label>
            <select id={ids.tier} value={filters.tier} onChange={(e) => set({ tier: e.target.value as TierFilter })}>
              <option value="all">All tiers</option>
              <option value="small">small</option>
              <option value="medium">medium</option>
              <option value="high">high</option>
              <option value="unrated">unrated</option>
            </select>
          </div>
          <div className="field">
            <label htmlFor={ids.policy}>Policy</label>
            <select id={ids.policy} value={filters.policy} onChange={(e) => set({ policy: e.target.value as PolicyFilter })}>
              <option value="all">All</option>
              <option value="allowed">Allowed</option>
              <option value="denied">Denied</option>
            </select>
          </div>
        </div>
      )}
      <Table caption="Model catalog" className="cat__table">
        <thead>
          <tr>
            {["#", "Model ID", "Tier", "Provider Kinds", "Reasoning / Coding Score", "Context", "Policy Status", "Ratings"].map((c) => (
              <th key={c} scope="col">
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 ? (
            <tr>
              <td colSpan={8} className="cat__empty">
                {models.models.length === 0 ? "The catalog is empty." : "No models match these filters."}
              </td>
            </tr>
          ) : (
            rows.map((m) => {
              const open = expanded === m.id;
              const detailId = `ag-rt-${m.id.replace(/[^\w-]/g, "_")}`;
              return (
                <Fragment key={m.id}>
                  <tr className={m.denied ? "is-denied" : undefined}>
                    <td className="mono muted">{String(models.models.indexOf(m) + 1).padStart(3, "0")}</td>
                    <th scope="row">
                      <span className={m.denied ? "mchip mchip--deny" : "mchip"}>{m.id}</span>
                    </th>
                    <td>{m.tier ? <Badge tone={TIER_TONE[m.tier] ?? "neutral"}>{m.tier}</Badge> : <Badge>unrated</Badge>}</td>
                    <td>{provider(m)}</td>
                    <td>
                      <span className="score-pair">
                        <Dots value={m.reasoning} label="Reasoning" />
                        <Dots value={m.coding} label="Coding" />
                      </span>
                    </td>
                    <td className="mono">{m.context_k === null ? "–" : `${m.context_k}k`}</td>
                    <td>
                      {m.denied ? (
                        <Badge tone="coral">
                          <Icon name="x" size={12} />
                          DENIED BY POLICY ({m.denied_by ?? "policy"})
                        </Badge>
                      ) : (
                        <Badge tone="emerald">
                          <Icon name="check" size={12} />
                          Allowed
                        </Badge>
                      )}
                    </td>
                    <td>
                      <Button size="sm" ghost aria-expanded={open} aria-controls={detailId} aria-label={`Ratings of ${m.id}`} onClick={() => setExpanded(open ? null : m.id)}>
                        {open ? "Hide" : "Show"}
                      </Button>
                    </td>
                  </tr>
                  {open && (
                    <tr id={detailId} className="cat__detail">
                      <td colSpan={8}>
                        <Ratings model={m} />
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })
          )}
        </tbody>
      </Table>
    </Card>
  );
}

/** What the policy keeps out of runs, and the tiers. Editing the policy is for the CLI for now. */
export function PolicyAndTiers({ models }: { models: ModelsResponse }) {
  const denied = models.models.filter((m) => m.denied);
  const inputId = useId();
  return (
    <>
      <Card as="section" className="sect" aria-labelledby="ag-h-policy">
        <CardHead title={<span id="ag-h-policy">Deny Policy Patterns</span>} icon={<Icon name="warning" />} />
        <div className="pol-ed">
          <p className="muted">
            A model whose id matches a pattern is never offered to the lead or given a role. <code>*</code> matches any run of
            characters.
            {models.policy.deny_why && <> Why: {models.policy.deny_why}</>}
          </p>
          <ul className="pats" aria-label="Deny patterns">
            {models.policy.deny.length ? (
              models.policy.deny.map((p) => (
                <li key={p} className="pat mono">
                  {p}
                </li>
              ))
            ) : (
              <li className="muted">No patterns. Every catalog model is allowed.</li>
            )}
          </ul>
          <p>
            {denied.length === 0
              ? "No model in the catalog is denied."
              : `${denied.length} model${denied.length === 1 ? " is" : "s are"} kept out of runs: ${denied.map((m) => m.id).join(", ")}.`}
          </p>
          <div className="pol-ed__add">
            <div className="field pol-ed__field">
              <label htmlFor={inputId}>Add pattern</label>
              <input id={inputId} className="mono" placeholder="*preview*" disabled />
            </div>
            <Button tone="coral" unavailable={`${NOT_AVAILABLE}: edit the policy with the orch CLI`}>
              <Icon name="plus" size={14} />
              Add to deny list
            </Button>
          </div>
        </div>
      </Card>
      <Card as="section" className="sect" aria-labelledby="ag-h-tiers">
        <CardHead title={<span id="ag-h-tiers">Tiers</span>} icon={<Icon name="chart" />} />
        <ul className="tiers">
          {models.tiers.map((t) => (
            <li key={t.name}>
              <Badge tone={TIER_TONE[t.name] ?? "neutral"}>{t.name}</Badge>
              <span className="muted">needs reasoning {t.reasoning_floor}+</span>
              {t.purpose && <span>{t.purpose}</span>}
            </li>
          ))}
        </ul>
      </Card>
    </>
  );
}
