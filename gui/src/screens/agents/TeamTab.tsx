import type { AgentsResponse, PlanResponse, StatusResponse } from "../../api/types";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card, CardHead } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { Table } from "../../ui/Table";
import { tierRank } from "./advice";
import { NOT_AVAILABLE } from "./notAvailable";

export interface TeamTabProps {
  data: AgentsResponse;
  status: StatusResponse | null;
  plan: PlanResponse | null;
  onOpenOutput: (agentName: string) => void;
}

/** The run's team (who holds which role, on which model) and which tier each milestone needs. */
export function TeamTab({ data, status, plan, onOpenOutput }: TeamTabProps) {
  const busy = new Map((status?.workers ?? []).map((w) => [w.name, w.task]));
  const team = data.team;
  return (
    <>
      <Card as="section" className="sect" aria-labelledby="ag-h-roles">
        <CardHead title={<span id="ag-h-roles">Role Assignments ({team.length})</span>} icon={<Icon name="robot" />} />
        {team.length === 0 ? (
          <p className="muted sect__empty">There is no run in this project yet, so there is no team. It is set when the lead plans a run.</p>
        ) : (
          <Table caption="Team">
            <thead>
              <tr>
                {["Name", "Role", "Kind", "Model", "Tier", "Working on / State", "Output"].map((c) => (
                  <th key={c} scope="col">
                    {c}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {team.map((m) => {
                const task = busy.get(m.name);
                const kind = data.agents.find((a) => a.kind === m.kind);
                return (
                  <tr key={m.name}>
                    <th scope="row">
                      <button type="button" className="ag-link mono" onClick={() => onOpenOutput(m.name)} aria-label={`Open output for ${m.name}`}>
                        {m.name}
                      </button>
                    </th>
                    <td>
                      <Badge tone={m.role === "lead" ? "emerald" : m.role === "reviewer" ? "violet" : "cyan"}>{m.role}</Badge>
                    </td>
                    <td className="mono">{m.kind}</td>
                    <td className="mono">{m.model ?? "–"}</td>
                    <td>{m.tier ?? "–"}</td>
                    <td>
                      {kind && !kind.installed ? (
                        <Badge tone="coral">not installed</Badge>
                      ) : m.working_on ? (
                        <Badge tone="cyan">
                          busy on {m.working_on.task} ({m.working_on.state})
                        </Badge>
                      ) : task ? (
                        <Badge tone="cyan">busy on {task}</Badge>
                      ) : status ? (
                        <Badge tone="emerald">free</Badge>
                      ) : (
                        <span className="muted">unknown</span>
                      )}
                    </td>
                    <td>
                      <Button size="sm" onClick={() => onOpenOutput(m.name)} aria-label={`View output for ${m.name}`}>
                        View output
                      </Button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        )}
      </Card>

      <Card as="section" className="sect" aria-labelledby="ag-h-routing">
        <CardHead title={<span id="ag-h-routing">Tier Routing</span>} icon={<Icon name="grid" />} />
        {!plan ? (
          <p className="muted sect__empty">There is no run in this project yet, so there is no plan to compare with the team.</p>
        ) : plan.milestones.length === 0 ? (
          <p className="muted sect__empty">The plan has no milestones.</p>
        ) : (
          <Table caption="Milestone tiers against the team">
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
                        <Badge tone="coral">nobody on the team can serve this tier</Badge>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        )}
      </Card>

      <Card as="section" className="sect tb__foot" aria-label="Change the team">
        <p className="muted">
          The API does not report which agents are benched, so benches are not shown. Adding, benching and assigning agents is
          done with the <code>orch</code> CLI.
        </p>
        <div className="tb__btns">
          <Button tone="emerald" solid unavailable={`${NOT_AVAILABLE}: assign roles with the orch CLI`}>
            <Icon name="check" size={14} />
            Apply team
          </Button>
        </div>
      </Card>
    </>
  );
}
