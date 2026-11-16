import { useState } from "react";
import type { PlanResponse, TaskInfo, TasksResponse } from "../../api/types";
import { Modal } from "../../ui/Modal";
import { ResourceView } from "../../ui/ResourceView";
import { useResource } from "../../ui/useResource";
import { StatusBadge } from "../../ui/StatusBadge";
import { MilestoneGraph } from "./Graph";
import { MilestoneDetail } from "./MilestoneDetail";
import { PlanApproval } from "./PlanApproval";
import "./workflows.css";

interface PlanAndTasks {
  plan: PlanResponse;
  tasks: TasksResponse;
}

function Facts({ entries }: { entries: [string, string | null | undefined][] }) {
  return (
    <dl className="plan-facts">
      {entries.map(([k, v]) =>
        v ? (
          <div key={k} className="plan-fact">
            <dt>{k}</dt>
            <dd>
              <code>{v}</code>
            </dd>
          </div>
        ) : null,
      )}
    </dl>
  );
}

/** One milestone in the list: the always-needed fields visible, the plan text in a toggle. */
function MilestoneCard({
  milestone,
  task,
  onOpen,
}: {
  milestone: PlanResponse["milestones"][number];
  task: TaskInfo | undefined;
  onOpen: () => void;
}) {
  return (
    <li className="ms-card" data-milestone={milestone.id}>
      <div className="ms-card-head">
        <button type="button" className="button ms-card-open" onClick={onOpen}>
          <code>{milestone.id}</code> {milestone.title}
        </button>
        <span className="ms-card-meta">
          <StatusBadge state={task?.state ?? "pending"} />
          {task?.worker && <span className="ms-card-worker">{task.worker}</span>}
          {task?.model && <code className="ms-card-model">{task.model}</code>}
          {milestone.tier && <span className="badge badge--muted">{milestone.tier}</span>}
          {milestone.estimate && <span className="badge badge--muted">{milestone.estimate}</span>}
          {!milestone.enabled && <span className="badge badge--muted">disabled</span>}
        </span>
      </div>
      <p className="ms-card-goal">{milestone.goal}</p>
      <details className="ms-card-more">
        <summary>Plan details</summary>
        <MilestoneDetail milestone={milestone} task={task} />
      </details>
    </li>
  );
}

/** The Workflows screen: the plan the lead wrote, its graph, and how the run stands. */
export function WorkflowsScreen() {
  const resource = useResource<PlanAndTasks>(
    (api, signal) => Promise.all([api.plan({ signal }), api.tasks({ signal })]).then(([plan, tasks]) => ({ plan, tasks })),
    [],
    { isEmpty: (d) => d.plan.milestones.length === 0, refreshOn: ["task-state", "run-state"] },
  );
  const [selected, setSelected] = useState<string | null>(null);

  const renderPlan = (data: PlanAndTasks) => {
    const { plan, tasks } = data;
    const taskOf = (id: string) => tasks.tasks.find((t) => t.id === id);
    const selectedMilestone = plan.milestones.find((m) => m.id === selected);
    const h = plan.header;
    return (
      <>
        <PlanApproval tasks={tasks.tasks} />
        <section className="panel" aria-labelledby="wf-header">
          <h2 id="wf-header">Plan settings</h2>
          {plan.title && <p className="plan-title">{plan.title}</p>}
          <Facts
            entries={[
              ["Branch", h.branch],
              ["Base", h.base],
              ["Check command", h.check_command],
              ["Setup command", h.setup_command],
              ["Isolation", h.isolation],
              ["Max parallel", h.max_parallel],
              ["Scope", h.scope],
              ["Budget", h.budget],
              ["Commits", h.commits],
              ["Lead", h.lead],
              ["Lead mode", h.lead_mode],
              ["Workers", h.workers],
              ["Reviewer", h.reviewer],
              ["Risk hold", h.risk_hold],
              ...h.extra.map((pair): [string, string] => [pair[0] ?? "", pair[1] ?? ""]),
            ]}
          />
        </section>

        <section className="panel" aria-labelledby="wf-breakdown">
          <h2 id="wf-breakdown">Feature breakdown</h2>
          <p className="prewrap">{plan.feature_breakdown}</p>
        </section>

        <section className="panel" aria-labelledby="wf-graph">
          <h2 id="wf-graph">Milestone graph</h2>
          <MilestoneGraph milestones={plan.milestones} tasks={tasks.tasks} onSelect={setSelected} />
          <ul className="graph-list" aria-label="Milestones as a list">
            {plan.milestones.map((m) => {
              const t = taskOf(m.id);
              return (
                <li key={m.id} className="graph-list-item">
                  <button type="button" className="button graph-list-open" onClick={() => setSelected(m.id)}>
                    <code>{m.id}</code> {m.title}
                  </button>
                  <StatusBadge state={t?.state ?? "pending"} />
                  <span className="muted graph-list-deps">
                    {m.depends_on.length > 0
                      ? `after ${m.depends_on.map((d) => (d.reason ? `${d.id} — ${d.reason}` : d.id)).join("; ")}`
                      : "no dependencies"}
                  </span>
                </li>
              );
            })}
          </ul>
        </section>

        <section aria-labelledby="wf-milestones">
          <h2 id="wf-milestones">Milestones</h2>
          <ul className="ms-cards">
            {plan.milestones.map((m) => (
              <MilestoneCard key={m.id} milestone={m} task={taskOf(m.id)} onOpen={() => setSelected(m.id)} />
            ))}
          </ul>
        </section>

        <Modal
          open={selected !== null && selectedMilestone !== undefined}
          title={selectedMilestone ? `${selectedMilestone.id}: ${selectedMilestone.title}` : ""}
          onClose={() => setSelected(null)}
        >
          {selectedMilestone && <MilestoneDetail milestone={selectedMilestone} task={taskOf(selectedMilestone.id)} />}
        </Modal>
      </>
    );
  };

  return (
    <section className="screen" aria-labelledby="screen-title">
      <h1 id="screen-title">Workflows</h1>
      <ResourceView
        resource={resource}
        empty="The plan has no milestones. Write them in .orchestrate/plan.md and run `orch sync`."
      >
        {renderPlan}
      </ResourceView>
    </section>
  );
}
