import { useId, useState, type KeyboardEvent, type ReactNode } from "react";
import type { PlanResponse } from "../../api/types";
import { useConnection } from "../../app/connection";
import { HerdrSetup, herdrRunningText, useHerdr } from "../../app/HerdrSetup";
import { useProjects } from "../../app/projects";
import type { ThemePreference } from "../../app/theme";
import { Badge } from "../../ui/Badge";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Icon, type IconName } from "../../ui/Icon";
import { ErrorPanel } from "../../ui/ResourceView";
import { Segmented } from "../../ui/Segmented";
import { useResource } from "../../ui/useResource";
import { SECTIONS, type Field } from "./sections";
import { useAppSettings, type AppSettingsState } from "./useAppSettings";
import { useProjectSettings, type ProjectSettingsState } from "./useProjectSettings";
import "./settings.css";

const NOT_AVAILABLE = "Not available yet";
const DESKTOP_ONLY = "Available in the desktop app";

const CAPS: [string, string, "cyan" | "violet" | "amber" | "coral"][] = [
  ["Fix", "3 rounds", "cyan"],
  ["Review", "2 rounds", "violet"],
  ["Conflict", "2 rounds", "amber"],
  ["Escalation", "1 step up", "coral"],
];

/** A row: what the setting is on the left, its value or control in the middle, where it lives on the right. */
function Row({ icon, label, desc, source, children }: { icon: IconName; label: string; desc: ReactNode; source: ReactNode; children: ReactNode }) {
  const id = useId();
  return (
    <div className="srow" role="group" aria-labelledby={id}>
      <span className="srow__icon">
        <Icon name={icon} />
      </span>
      <div className="srow__label">
        <div className="srow__name" id={id}>
          {label}
        </div>
        <p className="srow__desc">{desc}</p>
      </div>
      <div className="srow__ctl">{children}</div>
      <span className="src mono">{source}</span>
    </div>
  );
}

/** A setting the app cannot save yet: the control is there, disabled, and says why. */
function Unavailable({ label, control }: { label: string; control: Extract<Field, { kind: "none" }>["control"] }) {
  const title = `${NOT_AVAILABLE}: the app cannot save this setting`;
  const props = { disabled: true, title, "aria-label": `${label} (${NOT_AVAILABLE})` };
  return (
    <>
      {control === "area" ? (
        <textarea {...props} rows={2} placeholder={NOT_AVAILABLE} />
      ) : control === "text" || control === "number" ? (
        <input {...props} type={control} placeholder={NOT_AVAILABLE} />
      ) : (
        <select {...props}>
          <option>{NOT_AVAILABLE}</option>
        </select>
      )}
    </>
  );
}

function PlanValue({ plan, value, code }: { plan: PlanResponse | null; value: string | null | undefined; code?: boolean }) {
  if (!plan) return <span className="muted">No run yet</span>;
  if (!value) return <span className="muted">not set</span>;
  return code ? <code className="sval">{value}</code> : <span className="sval mono">{value}</span>;
}

function FieldRow({
  field,
  app,
  plan,
  agents,
  models,
  project,
}: {
  field: Field;
  app: AppSettingsState;
  plan: PlanResponse | null;
  agents: ReturnType<typeof useAgents>;
  models: ReturnType<typeof useModels>;
  project: ProjectSettingsState;
}) {
  const notifyId = useId();
  const orchId = useId();
  switch (field.kind) {
    case "app":
      if (field.id === "theme")
        return (
          <Row icon="moon" label="Theme" desc="Colour theme for the whole app. Applies at once; the top bar toggle changes the same setting." source="app settings">
            <Segmented<ThemePreference>
              label="Theme options"
              value={app.saved.theme}
              onChange={(v) => {
                if (!app.busy) void app.changeTheme(v);
              }}
              options={[
                { value: "dark", label: "Dark" },
                { value: "light", label: "Light" },
                { value: "system", label: "System" },
              ]}
            />
          </Row>
        );
      if (field.id === "notifications")
        return (
          <Row icon="bell" label="Desktop Notifications" desc="Questions, holds and the end of a run, shown by the system while the app is in the background." source="app settings">
            {app.backend ? (
              <label className="sswitch" htmlFor={notifyId}>
                <input id={notifyId} type="checkbox" role="switch" checked={app.saved.notifications} disabled={app.busy} onChange={(e) => void app.changeNotifications(e.target.checked)} />
                <span>Desktop notifications for questions, holds and the end of a run</span>
              </label>
            ) : (
              <span className="muted">{DESKTOP_ONLY}</span>
            )}
          </Row>
        );
      return (
        <Row
          icon="keyboard"
          label="orch Program"
          desc={
            app.detected ? (
              <>
                Found now: <code>{app.detected}</code>. Used the next time a project's server starts.
              </>
            ) : app.backend ? (
              "orch was not found: set its path here, or put it on the PATH."
            ) : (
              "Where the app finds orch."
            )
          }
          source="app settings"
        >
          {app.backend ? (
            <div className="sorch">
              <label className="visually-hidden" htmlFor={orchId}>
                Path of the orch binary
              </label>
              <input id={orchId} className="mono" value={app.orch} placeholder="Empty: use ORCH_BIN or the PATH" spellCheck={false} onChange={(e) => app.setOrch(e.target.value)} />
              <Button disabled={app.busy || app.orch === app.saved.orch_path} onClick={() => void app.saveOrchPath()}>
                Save path
              </Button>
            </div>
          ) : (
            <span className="muted">{DESKTOP_ONLY}</span>
          )}
        </Row>
      );
    case "herdr":
      return <HerdrRow />;
    case "plan":
      return (
        <Row icon={field.icon} label={field.label} desc={field.desc} source="plan.md">
          <PlanValue plan={plan} value={plan?.header[field.key]} code={field.code} />
        </Row>
      );
    case "env":
      return (
        <Row icon={field.icon} label={field.label} desc={field.desc} source={field.name}>
          <span className="sval mono">
            {field.def}
            {field.unit ? ` ${field.unit}` : ""}
          </span>
          <span className="muted sdefault">default; set {field.name} where orch runs to change it</span>
        </Row>
      );
    case "fact":
      return (
        <Row icon={field.icon} label={field.label} desc={field.desc} source="fixed">
          <code className="sval">{field.value}</code>
        </Row>
      );
    case "caps":
      return (
        <Row icon="sync" label="Bounded Retry Caps" desc="Shipped limits for retries and escalation (read-only)." source="fixed">
          <span className="caps">
            {CAPS.map(([k, v, tone]) => (
              <Badge key={k} tone={tone}>
                {k}: {v}
              </Badge>
            ))}
          </span>
        </Row>
      );
    case "agents":
      if (agents.state === "loading") return <p className="srow-note muted">Reading the agents…</p>;
      if (!agents.data) return <p className="srow-note muted">The agent list could not be read{agents.error ? `: ${agents.error.message}` : ""}.</p>;
      return (
        <>
          {agents.data.agents.map((a) => (
            <Row key={a.kind} icon="robot" label={a.display_name ?? a.kind} desc={<code>{a.launch}</code>} source={a.kind}>
              <Badge tone={a.installed ? "emerald" : "neutral"}>{a.installed ? "Installed" : "Not installed"}</Badge>
              <span className="muted">roles: {a.roles.join(", ") || "–"}</span>
              <select disabled title={`${NOT_AVAILABLE}: every installed agent can take a role`} aria-label={`Allow ${a.display_name ?? a.kind} (${NOT_AVAILABLE})`}>
                <option>{NOT_AVAILABLE}</option>
              </select>
            </Row>
          ))}
        </>
      );
    case "tiers":
      if (models.state === "loading") return <p className="srow-note muted">Reading the model catalog…</p>;
      if (!models.data) return <p className="srow-note muted">The model catalog could not be read{models.error ? `: ${models.error.message}` : ""}.</p>;
      return (
        <>
          {models.data.tiers.map((t) => (
            <Row key={t.name} icon="cube" label={`Tier: ${t.name}`} desc={t.purpose ?? `Needs reasoning ${t.reasoning_floor}+.`} source="models catalog">
              <span className="sval mono">reasoning {t.reasoning_floor}+</span>
            </Row>
          ))}
          <Row icon="warning" label="Deny Policy" desc={models.data.policy.deny_why ?? "Models matching these patterns are never used."} source="models catalog">
            {models.data.policy.deny.length ? (
              models.data.policy.deny.map((p) => (
                <Badge key={p} tone="coral" mono>
                  {p}
                </Badge>
              ))
            ) : (
              <span className="muted">No model is denied</span>
            )}
          </Row>
        </>
      );
    case "extra":
      if (!plan) return <p className="srow-note muted">No run yet: other plan header lines appear here once it exists.</p>;
      if (plan.header.extra.length === 0) return <p className="srow-note muted">The plan header has no other lines.</p>;
      return (
        <>
          {plan.header.extra.map(([k, v], i) => (
            <Row key={`${k}-${i}`} icon="keyboard" label={k ?? ""} desc="A plan header line." source="plan.md">
              <span className="sval mono">{v}</span>
            </Row>
          ))}
        </>
      );
    case "project": {
      if (!project.projectPath) {
        return (
          <Card className="set-note" role="status">
            <p className="panel-title">No project is open</p>
            <p className="muted">
              Open a project to configure its settings.
            </p>
          </Card>
        );
      }
      if (project.loading) {
        return (
          <p className="srow-note muted" role="status">
            Loading project settings…
          </p>
        );
      }
      if (project.loadError) {
        return (
          <Card className="set-note tone-coral" role="alert">
            <p className="panel-title">Cannot load project settings</p>
            <p>{project.loadError}</p>
            <div className="button-row" style={{ marginTop: 12 }}>
              <Button onClick={() => void project.reload()}>Try again</Button>
            </div>
          </Card>
        );
      }

      const isDesktop = Boolean(project.backend);
      const leadId = "st-lead-agent";
      const folderId = "st-project-folder";
      const missingSavedAgent =
        project.isSavedLeadMissing &&
        !project.clis.some((c) => c.kind === project.savedLeadAgent)
          ? project.savedLeadAgent
          : null;

      const leadDisplayName =
        project.clis.find((c) => c.kind === project.savedLeadAgent)?.displayName ??
        project.savedLeadAgent;

      return (
        <>
          {project.loadWarning && (
            <p className="set-msg tone-coral" role="alert">
              {project.loadWarning}
            </p>
          )}
          <Row
            icon="robot"
            label="Default Lead Agent"
            desc="The agent CLI used to plan and run milestones in this project."
            source={isDesktop ? "project settings" : NOT_AVAILABLE}
          >
            <div className="slead">
              <label className="visually-hidden" htmlFor={leadId}>
                Default Lead Agent
              </label>
              <select
                id={leadId}
                aria-label="Default Lead Agent"
                value={project.leadAgent}
                disabled={!isDesktop || project.busy}
                title={isDesktop ? undefined : DESKTOP_ONLY}
                onChange={(e) => project.setLeadAgent(e.target.value)}
              >
                <option value="">None (no default lead agent)</option>
                {project.clis.map((c) => (
                  <option
                    key={c.kind}
                    value={c.kind}
                    disabled={!c.installed || !c.canLead}
                  >
                    {c.displayName} ({c.installed ? "Installed" : "Not installed"})
                  </option>
                ))}
                {missingSavedAgent && (
                  <option value={missingSavedAgent} disabled>
                    {missingSavedAgent} (Not installed)
                  </option>
                )}
              </select>
              {project.isSavedLeadMissing && (
                <p className="field-hint tone-amber" role="alert">
                  The saved Lead agent “{leadDisplayName}” is not installed. Pick another Lead agent.
                </p>
              )}
            </div>
          </Row>

          <Row
            icon="folder"
            label="Project Folder"
            desc="Location of the git repository for this project."
            source={isDesktop ? "project settings" : NOT_AVAILABLE}
          >
            <div className="sfolder">
              <label className="visually-hidden" htmlFor={folderId}>
                Project Folder
              </label>
              <input
                id={folderId}
                aria-label="Project Folder"
                className="mono"
                value={project.folder}
                spellCheck={false}
                disabled={!isDesktop || project.busy}
                title={isDesktop ? undefined : DESKTOP_ONLY}
                onChange={(e) => project.setFolder(e.target.value)}
              />
              {project.folderError && (
                <p className="field-error tone-coral" role="alert">
                  {project.folderError}
                </p>
              )}
            </div>
          </Row>
        </>
      );
    }
    case "none":
      return (
        <Row icon={field.icon} label={field.label} desc={field.desc} source={NOT_AVAILABLE}>
          <Unavailable label={field.label} control={field.control} />
        </Row>
      );
  }
}

/** The app's Herdr: running (started by the app, or found running), or what to do about it. */
function HerdrRow() {
  const { shell } = useConnection();
  const herdr = useHerdr(shell);
  const label = "Herdr";
  if (!shell) {
    return (
      <Row icon="keyboard" label={label} desc="The desktop app runs Herdr in the background." source="runtime">
        <span className="muted">{DESKTOP_ONLY}</span>
      </Row>
    );
  }
  const info = herdr.info;
  if (!info) {
    return (
      <Row icon="keyboard" label={label} desc="Checking Herdr…" source="runtime">
        <Badge tone="cyan">Checking</Badge>
      </Row>
    );
  }
  if (info.state === "running") {
    return (
      <Row icon="keyboard" label={label} desc={herdrRunningText(info)} source="runtime">
        <Badge tone="emerald">{info.managed ? "Running (started by the app)" : "Running"}</Badge>
      </Row>
    );
  }
  const word = { missing: "Not installed", too_old: "Update needed", failed: "Not running" } as const;
  return (
    <Row icon="keyboard" label={label} desc="Orchestra needs Herdr to run the Lead and its agents." source="runtime">
      <Badge tone="amber">{word[info.state]}</Badge>
      <HerdrSetup shell={shell} info={info} onChange={herdr.setInfo} onRecheck={herdr.refresh} />
    </Row>
  );
}

const useAgents = () => useResource((api, signal) => api.agents({ signal }), []);
const useModels = () => useResource((api, signal) => api.models({ signal }), []);

/** Screen 08: every section of the prototype; app settings save, run settings are read-only. */
export function SettingsScreen() {
  const plan = useResource((api, signal) => api.plan({ signal }), []);
  const agents = useAgents();
  const models = useModels();
  const app = useAppSettings();
  const connection = useConnection();
  const pj = useProjects();
  const [stickyPath, setStickyPath] = useState<string | null>(null);
  const rawPath = pj.multi
    ? pj.selected
    : (pj.selected ?? connection.health?.project ?? null);
  const activePath = rawPath && rawPath.trim() ? rawPath.trim() : (connection.kind === "checking" ? stickyPath : null);
  if (rawPath && rawPath.trim() && rawPath.trim() !== stickyPath) {
    setStickyPath(rawPath.trim());
  }
  const project = useProjectSettings(activePath, {
    onFolderChanged: (newFolder) => {
      if (pj.multi) {
        if (activePath) {
          void pj.close(activePath).catch(() => {});
        }
        void pj.openListed(newFolder).catch(() => {});
      } else if (app.backend) {
        void app.backend
          .switchProject(newFolder)
          .then((conn) => connection.connect(conn))
          .catch(() => {});
      }
    },
  });
  const [current, setCurrent] = useState(SECTIONS[0]!.id);
  const noRun = plan.error?.isNoRun === true;
  const section = SECTIONS.find((s) => s.id === current) ?? SECTIONS[0]!;

  const go = (id: string) => {
    setCurrent(id);
    document.getElementById(`st-tab-${id}`)?.focus();
  };
  const onKey = (e: KeyboardEvent) => {
    const i = SECTIONS.findIndex((s) => s.id === current);
    const n = SECTIONS.length;
    const next =
      e.key === "ArrowDown" || e.key === "ArrowRight" ? (i + 1) % n : e.key === "ArrowUp" || e.key === "ArrowLeft" ? (i - 1 + n) % n : e.key === "Home" ? 0 : e.key === "End" ? n - 1 : null;
    if (next === null) return;
    e.preventDefault();
    go(SECTIONS[next]!.id);
  };

  return (
    <section className="screen set" aria-labelledby="screen-title">
      <header className="set__head">
        <span className="set__icon">
          <Icon name="gear" />
        </span>
        <div>
          <h1 id="screen-title" className="set__title">
            Settings
          </h1>
          <p className="set__sub">Configure Herdr Orchestra runtime, agents, models and execution policy.</p>
        </div>
      </header>

      {noRun && (
        <Card className="set-note" role="status">
          <p className="panel-title">No run in this project yet</p>
          <p className="muted">The run's settings appear here once the lead has planned a run. The app settings work already.</p>
        </Card>
      )}
      {plan.state === "error" && plan.error && !noRun && <ErrorPanel error={plan.error} onRetry={plan.reload} />}
      {app.loadWarning && (
        <p className="set-msg tone-coral" role="alert">
          {app.loadWarning}
        </p>
      )}

      <div className="set__body">
        <div className="set__navwrap">
          <div className="set__nav" role="tablist" aria-orientation="vertical" aria-label="Settings sections" onKeyDown={onKey}>
            {SECTIONS.map((s) => (
              <button
                key={s.id}
                id={`st-tab-${s.id}`}
                type="button"
                role="tab"
                className="navtab"
                aria-selected={s.id === current}
                aria-controls="st-panel"
                tabIndex={s.id === current ? 0 : -1}
                onClick={() => setCurrent(s.id)}
              >
                <Icon name={s.icon} />
                <span className="navtab__label">{s.label}</span>
              </button>
            ))}
          </div>
        </div>

        <Card as="section" className="spanel" id="st-panel" role="tabpanel" aria-labelledby="st-panel-title">
          <header className="spanel__head">
            <span className="spanel__icon">
              <Icon name={section.icon} />
            </span>
            <div className="spanel__titles">
              <h2 className="spanel__title" id="st-panel-title">
                {section.title}
              </h2>
              <p className="spanel__desc">{section.desc}</p>
            </div>
            <span className="prec mono">
              {section.id === "project" ? "Source: project settings" : "Sources: app settings · plan.md · environment"}
            </span>
          </header>
          {plan.state === "loading" && (
            <p className="srow-note muted" role="status">
              Loading…
            </p>
          )}
          {!app.loaded && (
            <p className="srow-note muted" role="status">
              Loading the app settings…
            </p>
          )}
          <div className="spanel__rows">
            {section.fields.map((f, i) => (
              <FieldRow key={i} field={f} app={app} plan={plan.data} agents={agents} models={models} project={project} />
            ))}
          </div>
          {app.outcome && section.id !== "project" && (
            <p className={app.outcome.kind === "ok" ? "set-msg tone-emerald" : "set-msg tone-coral"} role={app.outcome.kind === "ok" ? "status" : "alert"}>
              {app.outcome.text}
            </p>
          )}
          {section.id === "project" && project.saveError && !project.folderError && !project.loadError && (
            <p className="set-msg tone-coral" role="alert">
              {project.saveError}
            </p>
          )}
          {section.id === "project" && project.saveSuccess && (
            <p className="set-msg tone-emerald" role="status">
              {project.saveSuccess}
            </p>
          )}
          <footer className="spanel__foot">
            {section.id === "project" ? (
              project.projectPath && !project.loading && !project.loadError ? (
                <>
                  <Button
                    tone="emerald"
                    solid
                    disabled={!project.backend || project.busy || !project.isDirty}
                    unavailable={project.backend ? undefined : DESKTOP_ONLY}
                    onClick={() => void project.save()}
                  >
                    Save project settings
                  </Button>
                  <span className="muted spanel__hint">
                    {project.backend
                      ? "Saved per project in the shell's config folder. Never written into the project."
                      : DESKTOP_ONLY}
                  </span>
                </>
              ) : null
            ) : (
              <>
                <Button unavailable={`${NOT_AVAILABLE}: the run's settings come from plan.md`}>
                  <Icon name="sync" size={14} />
                  Reset to Defaults
                </Button>
                <span className="muted spanel__hint">App settings save as you change them. Run settings come from the plan and are read-only here.</span>
                <Button tone="emerald" solid unavailable={`${NOT_AVAILABLE}: the app does not write config.toml`}>
                  Save to config.toml
                </Button>
              </>
            )}
          </footer>
        </Card>
      </div>
    </section>
  );
}
