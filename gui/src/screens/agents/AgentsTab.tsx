import { useState } from "react";
import type { AgentKindInfo, AgentsResponse, ModelsResponse, TeamMember } from "../../api/types";
import { Badge } from "../../ui/Badge";
import { Button, type Tone } from "../../ui/Button";
import { Card, CardHead } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { Modal } from "../../ui/Modal";
import { agentProblems } from "./advice";
import { NOT_AVAILABLE } from "./notAvailable";

type Flag = [text: string, tone: Tone, icon: "check" | "warning" | "x"];

/** The status badges of an agent kind, in words: installed, login, skills, Herdr detection, flags. */
export function agentFlags(a: AgentKindInfo): Flag[] {
  if (!a.installed) return [["Not installed", "coral", "x"]];
  const out: Flag[] = [];
  if (a.login.state === "authenticated") out.push(["Logged In", "emerald", "check"]);
  else if (a.login.state === "not authenticated") out.push(["Logged Out", "coral", "x"]);
  else out.push(["Login Unknown", "neutral", "warning"]);
  if (a.skills.status === "current") out.push(["Skills Installed", "emerald", "check"]);
  else if (a.skills.status === "outdated") out.push(["Skills Outdated", "amber", "warning"]);
  else if (a.skills.status === "missing") out.push(["Skills Missing", "amber", "warning"]);
  out.push(a.herdr_detect ? ["Herdr Detect: ON", "emerald", "check"] : ["Herdr Detect: OFF", "amber", "warning"]);
  if (!a.verified) out.push(["Unverified Flags", "amber", "warning"]);
  return out;
}

const ROLE_WORD: Record<string, string> = { lead: "Lead", reviewer: "Reviewer" };

/** The run's team members of one kind, grouped by model: "Lead & Reviewer (opus)". */
export function assignedRoles(kind: string, team: TeamMember[]): string[] {
  const groups = new Map<string, string[]>();
  for (const m of team.filter((t) => t.kind === kind)) {
    const key = m.model ?? "default model";
    groups.set(key, [...(groups.get(key) ?? []), ROLE_WORD[m.role] ?? m.name]);
  }
  return [...groups].map(([model, who]) => `${who.join(" & ")} (${model})`);
}

function Flags({ a }: { a: AgentKindInfo }) {
  return (
    <span className="acard__badges">
      {agentFlags(a).map(([text, tone, icon]) => (
        <Badge key={text} tone={tone}>
          <Icon name={icon} size={12} />
          {text}
        </Badge>
      ))}
    </span>
  );
}

function AgentCard({ a, team, onOpen }: { a: AgentKindInfo; team: TeamMember[]; onOpen: () => void }) {
  const roles = assignedRoles(a.kind, team);
  const problems = agentProblems(a);
  const lead = team.some((t) => t.role === "lead" && t.kind === a.kind);
  const name = a.display_name ?? a.kind;
  return (
    <Card as="li" tone={lead ? "emerald" : !a.installed ? "coral" : undefined} className="acard">
      <span className="acard__logo" aria-hidden="true">
        <Icon name="robot" size={28} />
      </span>
      <div className="acard__main">
        <div className="acard__top">
          <button type="button" className="acard__name" aria-haspopup="dialog" aria-label={`${name} details`} onClick={onOpen}>
            {name} <span className="mono muted">({a.kind})</span>
          </button>
        </div>
        <Flags a={a} />
        <div className="acard__role">
          <Icon name="robot" size={16} />
          <span className="muted">Assigned Role:</span>
          {roles.length ? (
            roles.map((r) => (
              <Badge key={r} tone="violet" mono>
                {r}
              </Badge>
            ))
          ) : (
            <span className="muted">—</span>
          )}
        </div>
        <div className="acard__meta muted">
          Can be: {a.roles.join(", ") || "–"} · <code>{a.launch}</code>
        </div>
        {problems.length > 0 && (
          <ul className="acard__problems" aria-label={`Problems with ${a.kind}`}>
            {problems.map((p) => (
              <li key={p}>{p}</li>
            ))}
          </ul>
        )}
      </div>
    </Card>
  );
}

function AgentDetail({ a, team, models, onClose, onRecheck }: { a: AgentKindInfo; team: TeamMember[]; models: ModelsResponse | null; onClose: () => void; onRecheck: () => void }) {
  const roles = assignedRoles(a.kind, team);
  const usable = (models?.models ?? []).filter((m) => m.kinds.includes(a.kind));
  return (
    <Modal open title={a.display_name ?? a.kind} onClose={onClose}>
      <dl className="ag-dl">
        <dt>Launch</dt>
        <dd>
          <code>{a.launch}</code>
        </dd>
        <dt>Status</dt>
        <dd>
          <Flags a={a} />
        </dd>
        {a.login.reason && (
          <>
            <dt>Login</dt>
            <dd>{a.login.reason}</dd>
          </>
        )}
        {a.skills.problems.length > 0 && (
          <>
            <dt>Skills</dt>
            <dd>{a.skills.problems.join(", ")}</dd>
          </>
        )}
        <dt>Can be</dt>
        <dd>{a.roles.join(", ") || "–"}</dd>
        <dt>Models</dt>
        <dd>
          {usable.length ? (
            usable.map((m) => (
              <span key={m.id} className={m.denied ? "mchip mchip--deny" : "mchip"} title={m.denied ? `Denied by ${m.denied_by ?? "policy"}` : "Allowed"}>
                {m.id}
              </span>
            ))
          ) : (
            <span className="muted">None in the catalog</span>
          )}
        </dd>
        <dt>Roles</dt>
        <dd>
          {roles.length ? (
            roles.map((r) => (
              <Badge key={r} tone="violet" mono>
                {r}
              </Badge>
            ))
          ) : (
            <span className="muted">Unassigned</span>
          )}
        </dd>
      </dl>
      {agentProblems(a).length > 0 && (
        <ul className="acard__problems">
          {agentProblems(a).map((p) => (
            <li key={p}>{p}</li>
          ))}
        </ul>
      )}
      <div className="modal-actions">
        <Button unavailable={`${NOT_AVAILABLE}: bench agents with the orch CLI`}>Bench agent</Button>
        <Button
          tone="cyan"
          onClick={() => {
            onRecheck();
            onClose();
          }}
        >
          <Icon name="sync" size={14} />
          Re-check
        </Button>
        <Button onClick={onClose}>Close</Button>
      </div>
    </Modal>
  );
}

/** The agent CLIs `orch` knows, with what each can do, how it stands and which role it holds. */
export function AgentsTab({ data, models, onRecheck }: { data: AgentsResponse; models: ModelsResponse | null; onRecheck: () => void }) {
  const [open, setOpen] = useState<string | null>(null);
  const installed = data.agents.filter((a) => a.installed).length;
  const shown = data.agents.find((a) => a.kind === open);
  return (
    <Card as="section" className="sect" aria-labelledby="ag-h-agents">
      <CardHead title={<span id="ag-h-agents">Discovered Agent CLIs ({data.agents.length})</span>} icon={<Icon name="cube" />}>
        <span className="sect__note">
          <span className={installed === data.agents.length ? "dot dot--emerald" : "dot dot--amber"} aria-hidden="true" />
          {installed} of {data.agents.length} installed • CLI discovery, auth &amp; capability status
        </span>
      </CardHead>
      {data.agents.length === 0 ? (
        <p className="muted sect__empty">orch knows no agent kinds.</p>
      ) : (
        <ul className="agrid" aria-label="Agent kinds">
          {data.agents.map((a) => (
            <AgentCard key={a.kind} a={a} team={data.team} onOpen={() => setOpen(a.kind)} />
          ))}
        </ul>
      )}
      {shown && <AgentDetail a={shown} team={data.team} models={models} onClose={() => setOpen(null)} onRecheck={onRecheck} />}
    </Card>
  );
}
