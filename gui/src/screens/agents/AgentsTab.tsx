import type { AgentKindInfo, AgentsResponse, StatusResponse } from "../../api/types";
import { agentProblems } from "./advice";

function Flag({ on, yes, no }: { on: boolean; yes: string; no: string }) {
  return <span className={on ? "badge badge--success" : "badge badge--warning"}>{on ? yes : no}</span>;
}

function loginBadge(a: AgentKindInfo) {
  if (!a.installed) return <span className="muted">–</span>;
  const tone = a.login.state === "authenticated" ? "success" : a.login.state === "unknown" ? "muted" : "danger";
  return <span className={`badge badge--${tone}`}>{a.login.state}</span>;
}

function skillsBadge(a: AgentKindInfo) {
  const tone = a.skills.status === "current" ? "success" : a.skills.status === "none" ? "muted" : "warning";
  return <span className={`badge badge--${tone}`}>{a.skills.status}</span>;
}

export function AgentsTab({ data, status }: { data: AgentsResponse; status: StatusResponse | null }) {
  const busy = new Map((status?.workers ?? []).map((w) => [w.name, w.task]));
  return (
    <>
      <section className="panel" aria-label="Agent kinds">
        <h2 className="panel-title">Agent kinds ({data.agents.length})</h2>
        <div className="ag-wrap">
          <table className="ag-table">
            <caption className="visually-hidden">Agent kinds</caption>
            <thead>
              <tr>
                <th scope="col">Agent</th>
                <th scope="col">Installed</th>
                <th scope="col">Roles</th>
                <th scope="col">Herdr can watch</th>
                <th scope="col">Login</th>
                <th scope="col">Skills</th>
                <th scope="col">Launch</th>
              </tr>
            </thead>
            <tbody>
              {data.agents.map((a) => {
                const problems = agentProblems(a);
                return (
                  <tr key={a.kind} className={problems.length ? "ag-row ag-row--problem" : "ag-row"}>
                    <th scope="row">
                      {a.display_name ?? a.kind}
                      {a.display_name && <span className="muted"> ({a.kind})</span>}
                      {problems.length > 0 && (
                        <ul className="ag-problems" aria-label={`Problems with ${a.kind}`}>
                          {problems.map((p) => (
                            <li key={p}>{p}</li>
                          ))}
                        </ul>
                      )}
                    </th>
                    <td>
                      <Flag on={a.installed} yes="installed" no="missing" />
                    </td>
                    <td>{a.roles.join(", ") || "–"}</td>
                    <td>{a.herdr_detect ? "yes" : "no"}</td>
                    <td>{loginBadge(a)}</td>
                    <td>{skillsBadge(a)}</td>
                    <td>
                      <code>{a.launch}</code>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </section>

      <section className="panel" aria-label="The run's team">
        <h2 className="panel-title">The run's team ({data.team.length})</h2>
        {data.team.length === 0 ? (
          <p className="muted">There is no run in this project yet, so there is no team. It is set when you run `orch sync`.</p>
        ) : (
          <div className="ag-wrap">
            <table className="ag-table">
              <caption className="visually-hidden">Team</caption>
              <thead>
                <tr>
                  <th scope="col">Name</th>
                  <th scope="col">Role</th>
                  <th scope="col">Kind</th>
                  <th scope="col">Model</th>
                  <th scope="col">Tier</th>
                  <th scope="col">Availability</th>
                </tr>
              </thead>
              <tbody>
                {data.team.map((m) => {
                  const task = busy.get(m.name);
                  const kind = data.agents.find((a) => a.kind === m.kind);
                  return (
                    <tr key={m.name}>
                      <th scope="row">{m.name}</th>
                      <td>{m.role}</td>
                      <td>{m.kind}</td>
                      <td>{m.model ?? "–"}</td>
                      <td>{m.tier ?? "–"}</td>
                      <td>
                        {kind && !kind.installed ? (
                          <span className="badge badge--danger">not installed</span>
                        ) : m.role !== "worker" ? (
                          <span className="muted">–</span>
                        ) : task ? (
                          <span className="badge badge--info">busy on {task}</span>
                        ) : status ? (
                          <span className="badge badge--success">free</span>
                        ) : (
                          <span className="muted">unknown</span>
                        )}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
        <p className="muted">
          The API does not report which agents are benched, so benches are not shown. Adding or removing agents and
          scanning for them is done with the `orch` CLI.
        </p>
      </section>
    </>
  );
}
