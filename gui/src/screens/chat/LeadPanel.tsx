import { useId } from "react";
import { Link } from "react-router-dom";
import { useConnection } from "../../app/connection";
import { HerdrSetup, useHerdr } from "../../app/HerdrSetup";
import { Button } from "../../ui/Button";
import { Icon } from "../../ui/Icon";
import { CHOOSE_AGENT_TEXT, NO_AGENT_TEXT, type LeadControl } from "./useLead";

const STATUS_LABEL: Record<string, string> = { idle: "Idle", working: "Working", blocked: "Blocked" };
const STATUS_TONE: Record<string, string> = { idle: "neutral", working: "info", blocked: "danger" };

/**
 * Where the Lead stands, in words: running (its agent and status, kept live by the stream),
 * starting, or not there with what is needed to start one. Nothing here is an error unless a
 * start failed; a project with no Lead is a normal state. When the app's Herdr is missing, too
 * old or not running, it says so and offers the way out (install, update, Herdr's site).
 */
export function LeadPanel({ lead }: { lead: LeadControl }) {
  const id = useId();
  const { phase, view } = lead;
  const { shell } = useConnection();
  // asked again after a failed start: Herdr may be what failed
  const herdr = useHerdr(shell, lead.startError);
  const herdrBlocks = herdr.info !== null && (herdr.info.state === "missing" || herdr.info.state === "too_old");

  if (phase === "loading" || phase === "unknown") {
    if (!lead.readError) return null;
    return (
      <div className="lead-panel" data-lead="unknown">
        <p className="muted">The Lead's state could not be read: {lead.readError}</p>
        <Button onClick={lead.reload}>Check the Lead again</Button>
      </div>
    );
  }

  if (phase === "running") {
    const status = view?.status ?? null;
    const agent = lead.agents?.find((a) => a.kind === view?.agent)?.name ?? view?.agent ?? "an agent";
    return (
      <div className="lead-panel lead-panel--running" data-lead="running">
        <span className="lead-panel__line">
          <Icon name="robot" />
          <strong>Lead running</strong>
          <span className="muted">with {agent}</span>
        </span>
        <span className="lead-panel__status" role="status" aria-label={`Lead status: ${status ? (STATUS_LABEL[status] ?? status) : "not known"}`}>
          <span className={`badge badge--${status ? (STATUS_TONE[status] ?? "neutral") : "neutral"}`}>
            {status ? (STATUS_LABEL[status] ?? status) : "Status not known"}
          </span>
        </span>
        {view?.note && <span className="muted lead-panel__note">{view.note}</span>}
      </div>
    );
  }

  if (phase === "starting" || lead.starting) {
    const agent = lead.agentToUse?.name ?? view?.agent ?? "the agent";
    return (
      <div className="lead-panel" data-lead="starting">
        <p role="status">
          <strong>Starting</strong> the Lead with {agent}…
        </p>
        <Button solid tone="emerald" disabled aria-disabled="true">
          Starting…
        </Button>
        {view?.note && <p className="muted lead-panel__note">{view.note}</p>}
      </div>
    );
  }

  // none, or gone
  const choose = !lead.agentToUse;
  const none = lead.agents !== null && lead.agents.length === 0;
  const gone = phase === "gone";
  return (
    <div className="lead-panel" data-lead={phase}>
      <p className="lead-panel__title">
        <strong>{gone ? "The Lead has stopped" : "No Lead is running"}</strong>
        <span className="muted">
          {gone ? " Your conversation is kept. Start it again to carry on." : " Start it here, or just send your first message: that starts it."}
        </span>
      </p>
      {view?.note && <p className="muted lead-panel__note">{view.note}</p>}

      {lead.agents === null && <p className="muted" role="status">Checking which agents are installed…</p>}

      {none && (
        <p className="lead-panel__problem">
          {NO_AGENT_TEXT.replace(" See Settings.", "")}{" "}
          <Link to="/settings">Open Settings</Link> to see the agents and pick one once it is installed.
        </p>
      )}

      {lead.agents !== null && !none && choose && (
        <div className="field lead-panel__choose">
          <label htmlFor={`${id}-agent`}>
            {lead.savedMissing ? `The saved agent (${lead.savedAgent}) is not installed. Choose another to start the Lead` : "Choose the agent that starts the Lead"}
          </label>
          <select id={`${id}-agent`} value={lead.choice} onChange={(e) => lead.setChoice(e.target.value)}>
            <option value="">{CHOOSE_AGENT_TEXT.replace(" first.", "…")}</option>
            {lead.agents.map((a) => (
              <option key={a.kind} value={a.kind}>
                {a.name} (Installed)
              </option>
            ))}
          </select>
        </div>
      )}

      {lead.agentToUse && (
        <p className="muted lead-panel__uses">
          Will use <strong>{lead.agentToUse.name}</strong>
          {lead.agentToUse.kind === lead.savedAgent ? " (this project's default agent)" : ""}.
        </p>
      )}

      {herdr.info && (
        <HerdrSetup
          shell={shell}
          info={herdr.info}
          onChange={(info) => {
            herdr.setInfo(info);
            if (info.state === "running") lead.reload();
          }}
          onRecheck={herdr.refresh}
        />
      )}

      {lead.startError && (
        <p className="lead-panel__error" role="alert">
          <strong>The Lead did not start.</strong> {lead.startError}
        </p>
      )}

      {!none && (
        <Button solid tone="emerald" disabled={!lead.agentToUse || lead.agents === null || herdrBlocks} onClick={() => void lead.start()}>
          {lead.startError ? "Try again" : "Start Lead"}
        </Button>
      )}
    </div>
  );
}
