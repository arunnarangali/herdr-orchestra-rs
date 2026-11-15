import { useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";
import { useConnection } from "../../app/connection";
import { useProjects } from "../../app/projects";
import type { AgentsResponse } from "../../api/types";
import { CopyCommand } from "./CopyCommand";
import { checkGitRepository } from "./git";
import { readProgress, writeProgress, type Progress } from "./progress";
import { agentsResult, checking, finalCommands, herdrResult, orchResult, type StepResult } from "./steps";
import "./onboarding.css";

const STEPS = [
  { id: "orch", label: "orch" },
  { id: "herdr", label: "Herdr" },
  { id: "agents", label: "Agents" },
  { id: "project", label: "Project" },
  { id: "start", label: "Start" },
] as const;
type StepId = (typeof STEPS)[number]["id"];

const RESUME_KEY = "orch.onboarding.step";

/** The step to open at, once, after the frame sent the user away and back (see Flow). */
function takeResumeStep(): number {
  const v = Number(window.sessionStorage.getItem(RESUME_KEY));
  window.sessionStorage.removeItem(RESUME_KEY);
  return Number.isInteger(v) && v >= 0 && v < STEPS.length ? v : 0;
}

const STATE_WORD = { checking: "checking", ok: "done", problem: "needs attention", unknown: "not checked" } as const;

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** The first minutes: check orch, Herdr and agents, choose a project, then start with a lead. */
export function OnboardingScreen() {
  const [progress, setProgress] = useState<Progress>(readProgress);
  const [open, setOpen] = useState(progress === null);

  if (!open) {
    return (
      <section className="screen onboarding" aria-labelledby="screen-title">
        <h1 id="screen-title">Getting started</h1>
        <div className="panel ob-card">
          <p className="panel-title">{progress === "done" ? "Setup is complete." : "Setup was skipped."}</p>
          <p className="muted">
            {progress === "done"
              ? "You will not be walked through it again. You can run the checks any time."
              : "You can run the guided setup whenever you like; it is also in the Help menu."}
          </p>
          <button type="button" className="button button--primary" onClick={() => setOpen(true)}>
            {progress === "done" ? "Run the checks again" : "Start the guided setup"}
          </button>
        </div>
      </section>
    );
  }

  return (
    <Flow
      onFinish={() => {
        writeProgress("done");
        setProgress("done");
        setOpen(false);
      }}
      onSkip={() => {
        writeProgress("skipped");
        setProgress("skipped");
        setOpen(false);
      }}
    />
  );
}

function Flow({ onFinish, onSkip }: { onFinish: () => void; onSkip: () => void }) {
  const { shell, health, client, kind, connect, recheck } = useConnection();
  const { multi, openFolder } = useProjects();
  const navigate = useNavigate();
  const [index, setIndex] = useState(takeResumeStep);
  const heading = useRef<HTMLHeadingElement>(null);
  const [nonce, setNonce] = useState({ orch: 0, herdr: 0, agents: 0 });
  const rerun = (id: keyof typeof nonce) => setNonce((n) => ({ ...n, [id]: n[id] + 1 }));

  const [orch, setOrch] = useState<{ path: string | null; error: string | null } | null>(null);
  const [herdr, setHerdr] = useState<{ path: string | null; error: string | null } | null>(null);
  const [agents, setAgents] = useState<{ data: AgentsResponse | null; error: string | null } | null>(null);
  const [issue, setIssue] = useState<string | null>(null);
  const [unverified, setUnverified] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setOrch(null);
    if (!shell) return;
    let live = true;
    shell.findOrch().then(
      (path) => live && setOrch({ path, error: null }),
      (e: unknown) => live && setOrch({ path: null, error: messageOf(e) }),
    );
    return () => {
      live = false;
    };
  }, [shell, nonce.orch]);

  useEffect(() => {
    setHerdr(null);
    if (!shell) return;
    let live = true;
    shell.findHerdr().then(
      (path) => live && setHerdr({ path, error: null }),
      (e: unknown) => live && setHerdr({ path: null, error: messageOf(e) }),
    );
    return () => {
      live = false;
    };
  }, [shell, nonce.herdr]);

  // The frame leaves the connect step for the first screen as soon as a project is chosen.
  // The guide is not finished then, so come straight back to it at the project step.
  // Only for a project chosen with this guide's own button; the frame's connect form keeps its behaviour.
  const previousKind = useRef(kind);
  const fromGuide = useRef(false);
  useEffect(() => {
    if (previousKind.current === "unconfigured" && kind !== "unconfigured" && fromGuide.current) {
      fromGuide.current = false;
      window.sessionStorage.setItem(RESUME_KEY, String(STEPS.findIndex((s) => s.id === "project")));
      setTimeout(() => navigate("/onboarding", { replace: true }), 0);
    }
    previousKind.current = kind;
  }, [kind, navigate]);

  const connected = client !== null && (kind === "connected" || kind === "no-run");
  useEffect(() => {
    setAgents(null);
    if (!client || !connected) return;
    const abort = new AbortController();
    client.agents({ signal: abort.signal }).then(
      (data) => !abort.signal.aborted && setAgents({ data, error: null }),
      (e: unknown) => !abort.signal.aborted && setAgents({ data: null, error: messageOf(e) }),
    );
    return () => abort.abort();
  }, [client, connected, nonce.agents]);

  const results: Record<StepId, StepResult> = {
    orch: shell && !orch ? checking("Looking for orch…") : orchResult(orch?.path ?? null, orch?.error ?? null, health, shell !== null),
    herdr: shell && !herdr ? checking("Looking for Herdr…") : herdrResult(herdr?.path ?? null, herdr?.error ?? null, shell !== null),
    agents: agentsResult(agents?.data ?? null, connected, agents?.error ?? null),
    project: projectResult(),
    start: { state: "ok", found: "", next: "", commands: [] },
  };

  function projectResult(): StepResult {
    if (kind === "unconfigured") {
      return {
        state: "problem",
        found: issue ?? "No project is chosen yet.",
        next: shell
          ? "Choose the folder of your git repository. The app starts orch for it; it changes nothing in the folder."
          : "Start `orch serve` in your git repository and enter its address and token in the form above.",
        commands: shell ? [] : ["orch serve --port 8787"],
      };
    }
    if (kind === "no-server" || kind === "bad-token") {
      return {
        state: "problem",
        found: kind === "no-server" ? "The project's server is not answering." : "The server refused the token.",
        next: "See the message at the top of the window, then check again or choose the folder again.",
        commands: [],
      };
    }
    if (kind === "checking" || !health) return checking("Connecting to the project…");
    return {
      state: "ok",
      found:
        `Project: ${health.project}.` +
        (health.run ? " It has a run." : " It has no run yet; the lead writes the plan in the last step.") +
        (unverified ? " The app could not confirm it is a git repository: orch needs one." : ""),
      next: "Nothing to do. Choose another folder to change project.",
      commands: [],
    };
  }

  const choose = async () => {
    if (!shell) return;
    setBusy(true);
    setIssue(null);
    try {
      const folder = await shell.chooseProjectFolder();
      if (!folder) return;
      const git = await checkGitRepository(folder);
      if (git.kind === "no") {
        setIssue(
          `${folder} is not a git repository (${git.reason}). Choose the folder that contains .git, or run \`git init\` there first.`,
        );
        return;
      }
      setUnverified(git.kind === "unknown");
      fromGuide.current = true;
      if (multi) await openFolder(folder);
      else connect(await shell.startServer(folder));
    } catch (e) {
      setIssue(messageOf(e));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    heading.current?.focus();
  }, [index]);

  const step = STEPS[index] ?? STEPS[0];
  const r = results[step.id];
  const firstAgent = agents?.data?.agents.find((a) => a.installed)?.kind ?? null;
  const attention = STEPS.filter((s) => s.id !== "start" && results[s.id].state === "problem");
  const last = index === STEPS.length - 1;

  return (
    <section className="screen onboarding" aria-labelledby="screen-title">
      <h1 id="screen-title">Getting started</h1>
      <nav aria-label="Setup steps">
        <ol className="ob-indicator">
          {STEPS.map((s, i) => (
            <li key={s.id}>
              <button
                type="button"
                className="ob-step-btn"
                aria-current={i === index ? "step" : undefined}
                onClick={() => setIndex(i)}
              >
                <span>
                  {i + 1}. {s.label}
                </span>
                <span className={`badge badge--${results[s.id].state === "ok" ? "success" : results[s.id].state === "problem" ? "warning" : "muted"}`}>
                  {s.id === "start" ? "last" : STATE_WORD[results[s.id].state]}
                </span>
              </button>
            </li>
          ))}
        </ol>
      </nav>
      <p className="muted" role="status">
        Step {index + 1} of {STEPS.length}
      </p>

      <article className="panel ob-card" aria-labelledby="ob-step-title">
        <h2 id="ob-step-title" className="panel-title" tabIndex={-1} ref={heading}>
          {index + 1}. {titleOf(step.id)}
        </h2>

        {last ? (
          <>
            {attention.length > 0 ? (
              <p className="ob-msg" role="note">
                Still open: {attention.map((s) => s.label).join(", ")}. You can read on, and come back to those steps.
              </p>
            ) : (
              <p>Everything checked is in place.</p>
            )}
            <ol className="ob-list">
              <li>
                Prepare the team and open the panes in Herdr (the lead builds a team from what is installed):
                {finalCommands(firstAgent).map((c) => (
                  <CopyCommand key={c} command={c} />
                ))}
              </li>
              <li>Open a lead pane in Herdr (any installed agent) and describe the feature to the lead.</li>
              <li>Read the plan the lead writes: the Overview and Execution screens show it as it runs.</li>
              <li>Say <strong>go</strong>. The event manager runs the milestones and wakes the lead when you are needed.</li>
            </ol>
            <p className="muted">The app only shows these commands; it never runs them or installs anything.</p>
          </>
        ) : (
          <>
            <p className="ob-found" role="status">
              <span className={`badge badge--${r.state === "ok" ? "success" : r.state === "problem" ? "warning" : "muted"}`}>
                {STATE_WORD[r.state]}
              </span>{" "}
              {r.found}
            </p>
            {r.next && <p>{r.next}</p>}
            {r.commands.map((c) => (
              <CopyCommand key={c} command={c} />
            ))}
            {step.id === "agents" && agents?.data && agents.data.agents.some((a) => !a.installed) && (
              <ul className="ob-list" aria-label="Missing agents">
                {agents.data.agents
                  .filter((a) => !a.installed)
                  .map((a) => (
                    <li key={a.kind}>
                      {a.display_name ?? a.kind} is not installed; to use it, install it so <code>{a.launch}</code> works in a terminal.
                    </li>
                  ))}
              </ul>
            )}
            {step.id === "project" && shell && (
              <p>
                <button type="button" className="button button--primary" disabled={busy} onClick={() => void choose()}>
                  {busy ? "Starting…" : kind === "unconfigured" ? "Choose your git repository…" : "Choose another repository…"}
                </button>
              </p>
            )}
          </>
        )}

        <div className="ob-actions">
          {!last && step.id !== "start" && (
            <button
              type="button"
              className="button"
              onClick={() => (step.id === "project" ? recheck() : rerun(step.id as keyof typeof nonce))}
            >
              Check again
            </button>
          )}
          <span className="ob-spacer" />
          <button type="button" className="button" onClick={onSkip}>
            Skip setup
          </button>
          <button type="button" className="button" disabled={index === 0} onClick={() => setIndex(index - 1)}>
            Back
          </button>
          {last ? (
            <button type="button" className="button button--primary" onClick={onFinish}>
              Finish setup
            </button>
          ) : (
            <button type="button" className="button button--primary" onClick={() => setIndex(index + 1)}>
              Next
            </button>
          )}
        </div>
      </article>
    </section>
  );
}

function titleOf(id: StepId): string {
  switch (id) {
    case "orch":
      return "The orch program";
    case "herdr":
      return "Herdr";
    case "agents":
      return "Coding agents";
    case "project":
      return "Your project";
    case "start":
      return "Start your first task";
  }
}
