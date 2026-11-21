import { useState } from "react";
import type { TeamMember } from "../../api/types";
import { Badge } from "../../ui/Badge";
import { Button, type Tone } from "../../ui/Button";
import { Card, CardHead } from "../../ui/Card";
import { Icon } from "../../ui/Icon";
import { useResource } from "../../ui/useResource";
import { stripAnsi } from "./ansi";

const ROLE_LABEL: Record<string, string> = {
  worker: "Worker",
  reviewer: "Reviewer",
  lead: "Lead",
};

function stateLook(member: TeamMember): { label: string; tone: Tone; busy: boolean } {
  const state = member.working_on?.state;
  if (!state) return { label: "Idle", tone: "neutral", busy: false };
  const label = state.charAt(0).toUpperCase() + state.slice(1);
  const tone: Tone = state === "failed" ? "coral" : state === "held" ? "amber" : state === "review" || state === "reviewing" ? "violet" : "cyan";
  return { label, tone, busy: true };
}

function AgentRow({ member }: { member: TeamMember }) {
  const [open, setOpen] = useState(false);
  const output = useResource(
    (api, signal) =>
      open ? api.agentOutput(member.name, { lines: 120 }, { signal }) : Promise.resolve(null),
    [open, member.name],
  );
  const look = stateLook(member);
  const task = member.working_on?.task;
  const detail = task ? `${look.label} ${task}` : look.label;

  return (
    <li className={`agent agent--${member.role}`}>
      <button
        type="button"
        className="agent__head"
        aria-expanded={open}
        aria-controls={`agent-${member.name}-output`}
        onClick={() => setOpen((o) => !o)}
      >
        <span className={`dot ${look.busy ? "dot--emerald" : "dot--idle"}`} aria-hidden="true" />
        <span className="agent__icon">
          <Icon name={member.role === "lead" ? "robot" : member.role === "reviewer" ? "check" : "branch"} />
        </span>
        <span className="agent__who">
          <span className="agent__name">
            {member.name}
            <span className="mono">
              ({member.kind}
              {member.model ? `: ${member.model}` : ""})
            </span>
          </span>
          <span className="agent__detail">
            {ROLE_LABEL[member.role] ?? member.role}
            {task ? ` · ${task}` : ""}
            {member.tier ? ` · ${member.tier}` : ""}
          </span>
        </span>
        <Badge tone={look.tone}>{detail}</Badge>
        <span className="agent__chev" aria-hidden="true">
          {open ? "▴" : "▾"}
        </span>
      </button>

      {open && (
        <div className="agent__log" id={`agent-${member.name}-output`}>
          {output.state === "loading" && (
            <p role="status" className="muted">
              Reading {member.name}'s output…
            </p>
          )}
          {output.state === "error" && (
            <div className="muted agent__output-error" role="alert">
              Could not read this agent's output.
              <Button size="sm" onClick={output.reload}>
                Try again
              </Button>
            </div>
          )}
          {output.state === "ready" && output.data && (
            <>
              <pre className="agent__pre" tabIndex={0} aria-label={`${member.name} recent output`}>
                {stripAnsi(output.data.text)}
              </pre>
              <div className="agent__foot">
                <span>
                  read-only · ANSI stripped
                  {member.working_on ? ` · ${member.working_on.task}` : ""}
                </span>
                <Button
                  size="sm"
                  unavailable="Not available from the app yet: open the pane in Herdr."
                >
                  <Icon name="external" />
                  Open in Herdr
                </Button>
              </div>
            </>
          )}
        </div>
      )}
    </li>
  );
}

/** The team named in the plan and what each one is doing; output is read only when expanded. */
export function AgentsAtWork() {
  const agents = useResource((api, signal) => api.agents({ signal }), [], {
    refreshOn: ["task-state", "run-state"],
    refreshDelayMs: 300,
  });
  const team = agents.state === "ready" && agents.data ? agents.data.team : [];

  if (agents.state === "error") return null;

  return (
    <Card as="section" className="agents-panel" aria-labelledby="agents-title">
      <CardHead title={<span id="agents-title">Agents at Work</span>} icon={<Icon name="robot" />}>
        {agents.refreshing && <span className="muted agents-panel__refreshing">Refreshing…</span>}
      </CardHead>
      {agents.state === "loading" && (
        <p role="status" className="muted agents-panel__empty">
          Loading the team…
        </p>
      )}
      {agents.state === "ready" && team.length === 0 && (
        <p className="muted agents-panel__empty">No agents are running yet.</p>
      )}
      {team.length > 0 && (
        <ul className="agents" aria-label="Agents at work">
          {team.map((m) => (
            <AgentRow key={m.name} member={m} />
          ))}
        </ul>
      )}
    </Card>
  );
}
