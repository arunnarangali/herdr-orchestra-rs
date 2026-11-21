import { useEffect, useRef, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { useConnection } from "../../app/connection";
import { useProjects } from "../../app/projects";
import type { AgentsResponse } from "../../api/types";
import { CopyCommand } from "./CopyCommand";
import { checkGitRepository } from "./git";
import { readProgress, writeProgress, type Progress } from "./progress";
import { Badge } from "../../ui/Badge";
import { Button, buttonClass, type Tone } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Icon, type IconName } from "../../ui/Icon";
import { agentsResult, checking, finalCommands, herdrResult, orchResult, skillsResult, type StepResult } from "./steps";
import "./onboarding.css";

const STEPS = [
  { id: "orch", label: "orch", icon: "keyboard", desc: "Check that the orch CLI is installed and available in your PATH." },
  { id: "herdr", label: "Herdr", icon: "pulse", desc: "Check that the Herdr terminal runtime is installed." },
  { id: "agents", label: "Agents", icon: "robot", desc: "Detect the coding-agent CLIs and their login state." },
  { id: "skills", label: "Skills", icon: "cube", desc: "Make sure the orch skill wrappers are installed for every agent." },
  { id: "project", label: "Project", icon: "branch", desc: "Choose your project: an existing git repository." },
  { id: "start", label: "Start", icon: "rocket", desc: "Prepare the team and start the orchestration lead." },
] as const satisfies readonly { id: string; label: string; icon: IconName; desc: string }[];
type StepId = (typeof STEPS)[number]["id"];

const RESUME_KEY = "orch.onboarding.step";

/** The step to open at, once, after the frame sent the user away and back (see Flow). */
function takeResumeStep(): number {
  const v = Number(window.sessionStorage.getItem(RESUME_KEY));
  window.sessionStorage.removeItem(RESUME_KEY);
  return Number.isInteger(v) && v >= 0 && v < STEPS.length ? v : 0;
}

const STATE_WORD = { checking: "checking", ok: "done", problem: "needs attention", unknown: "not checked" } as const;
const STATE_TONE: Record<StepResult["state"], Tone> = { checking: "cyan", ok: "emerald", problem: "amber", unknown: "neutral" };
const STATE_HEAD: Record<StepResult["state"], string> = { checking: "Checking…", ok: "Passed", problem: "Action Needed", unknown: "Not checked" };

function messageOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** The first minutes: check orch, Herdr and agents, choose a project, then start with a lead. */
export function OnboardingScreen() {
  const [progress, setProgress] = useState<Progress>(readProgress);
  const [open, setOpen] = useState(progress === null);

  if (!open) {
    return (
      <section className="screen ob" aria-labelledby="screen-title">
        <Head />
        <Card tone={progress === "done" ? "emerald" : undefined} className="ob__done">
          <Icon name={progress === "done" ? "check" : "rocket"} size={30} />
          <div className="ob__done-text">
            <p className="panel-title">{progress === "done" ? "Setup is complete." : "Setup was skipped."}</p>
            <p className="muted">
              {progress === "done"
                ? "You will not be walked through it again. You can run the checks any time."
                : "You can run the guided setup whenever you like; Onboarding stays in the sidebar."}
            </p>
          </div>
          <div className="ob__done-actions">
            <Button tone={progress === "done" ? undefined : "violet"} onClick={() => setOpen(true)}>
              {progress === "done" ? "Run the checks again" : "Start the guided setup"}
            </Button>
            <Link className={buttonClass({ tone: "emerald", solid: true })} to="/chat">
              <Icon name="chat" size={14} />
              Go to Chat / Home
            </Link>
          </div>
        </Card>
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

function Head({ children }: { children?: React.ReactNode }) {
  return (
    <header className="ob__head">
      <span className="ob__icon">
        <Icon name="rocket" />
      </span>
      <div className="ob__who">
        <h1 id="screen-title" className="ob__title">
          Getting started
        </h1>
        <p className="ob__sub">Workspace preflight &amp; environment check. Nothing is installed automatically: verify your local toolchain below.</p>
      </div>
      {children}
    </header>
  );
}

function Flow({ onFinish, onSkip }: { onFinish: () => void; onSkip: () => void }) {
  const { shell, health, client, kind, connect, recheck } = useConnection();
  const { multi, openFolder } = useProjects();
  const navigate = useNavigate();
  const [index, setIndex] = useState(takeResumeStep);
  const heading = useRef<HTMLHeadingElement>(null);
  const [nonce, setNonce] = useState({ orch: 0, herdr: 0, agents: 0, skills: 0 });
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
  }, [client, connected, nonce.agents, nonce.skills]);

  const results: Record<StepId, StepResult> = {
    orch: shell && !orch ? checking("Looking for orch…") : orchResult(orch?.path ?? null, orch?.error ?? null, health, shell !== null),
    herdr: shell && !herdr ? checking("Looking for Herdr…") : herdrResult(herdr?.path ?? null, herdr?.error ?? null, shell !== null),
    agents: agentsResult(agents?.data ?? null, connected, agents?.error ?? null),
    skills: skillsResult(agents?.data ?? null, connected, agents?.error ?? null),
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

  const checks = STEPS.filter((s) => s.id !== "start");
  const passed = checks.filter((s) => results[s.id].state === "ok").length;
  const rerunAll = () => {
    setNonce((n) => ({ orch: n.orch + 1, herdr: n.herdr + 1, agents: n.agents + 1, skills: n.skills + 1 }));
    recheck();
  };

  return (
    <section className="screen ob" aria-labelledby="screen-title">
      <Head>
        <div className="ob__head-actions">
          <Button onClick={rerunAll}>
            <Icon name="sync" size={14} />
            Re-run All Checks
          </Button>
        </div>
      </Head>

      <Card as="section" className="ob__bar" aria-label="Progress">
        <div className="ob__progress">
          <strong>
            {passed} of {checks.length} checks passed
          </strong>
          <div className="ob__track" role="progressbar" aria-label="Preflight progress" aria-valuemin={0} aria-valuemax={checks.length} aria-valuenow={passed}>
            <i style={{ width: `${(passed / checks.length) * 100}%` }} />
          </div>
          <span className="muted" role="status">
            Step {index + 1} of {STEPS.length}
          </span>
        </div>
        <div className="ob__nav">
          <Button disabled={index === 0} onClick={() => setIndex(index - 1)}>
            Back
          </Button>
          <Button onClick={onSkip}>Skip setup</Button>
          {last ? (
            <Button tone="emerald" solid onClick={onFinish}>
              Finish setup
            </Button>
          ) : (
            <Button tone="violet" onClick={() => setIndex(index + 1)}>
              Next
            </Button>
          )}
        </div>
      </Card>

      <div className="ob__grid">
        <nav aria-label="Setup steps">
          <ol className="steps">
            {STEPS.map((s, i) => {
              const st = results[s.id].state;
              const tone = s.id === "start" ? "violet" : STATE_TONE[st];
              return (
                <li key={s.id} className={`step tone-${tone}${i === index ? " step--on" : ""}`}>
                  <span className="step__no" aria-hidden="true">
                    {i + 1}
                  </span>
                  <Card className="step__card">
                    <span className="step__icon">
                      <Icon name={s.icon} />
                    </span>
                    <div className="step__main">
                      <button type="button" className="ob-step-btn" aria-current={i === index ? "step" : undefined} onClick={() => setIndex(i)}>
                        <span>
                          {i + 1}. {s.label}
                        </span>
                        <Badge tone={tone}>{s.id === "start" ? "last" : STATE_WORD[st]}</Badge>
                      </button>
                      <p className="step__desc">{s.desc}</p>
                    </div>
                    {s.id !== "start" && (
                      <p className="step__state">
                        <strong>{STATE_HEAD[st]}</strong>
                      </p>
                    )}
                  </Card>
                </li>
              );
            })}
          </ol>
        </nav>

        <Card as="article" tone={step.id === "start" ? "violet" : r.state === "problem" ? "amber" : undefined} className="ob-card" aria-labelledby="ob-step-title">
          <h2 id="ob-step-title" className="ob-card__title" tabIndex={-1} ref={heading}>
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
                <li>
                  Say <strong>go</strong>. The event manager runs the milestones and wakes the lead when you are needed.
                </li>
              </ol>
              <p className="muted">The app only shows these commands; it never runs them or installs anything. Finish setup, then chat with the lead from Chat / Home.</p>
            </>
          ) : (
            <>
              <p className="ob-found" role="status">
                <Badge tone={STATE_TONE[r.state]}>{STATE_WORD[r.state]}</Badge> {r.found}
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
                  <Button tone="emerald" solid disabled={busy} onClick={() => void choose()}>
                    <Icon name="folder" size={14} />
                    {busy ? "Starting…" : kind === "unconfigured" ? "Choose your git repository…" : "Choose another repository…"}
                  </Button>
                </p>
              )}
              <div className="ob-actions">
                <Button onClick={() => (step.id === "project" ? recheck() : rerun(step.id as keyof typeof nonce))}>
                  <Icon name="sync" size={14} />
                  Check again
                </Button>
              </div>
            </>
          )}
        </Card>
      </div>
    </section>
  );
}

function titleOf(id: StepId): string {
  switch (id) {
    case "orch":
      return "Orchestra Binary (orch)";
    case "herdr":
      return "Herdr Terminal Runtime";
    case "agents":
      return "Installed AI Coding Agents";
    case "skills":
      return "Orchestration Skill Wrappers";
    case "project":
      return "Git Repository & Project";
    case "start":
      return "Start the Lead";
  }
}
