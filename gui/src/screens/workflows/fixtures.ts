import type { PlanCriterion, PlanDependency, PlanHeader, PlanMilestone, PlanResponse, ReportResponse, TaskInfo, TasksResponse } from "../../api/types";

/**
 * Fixture plans and runs for the screen tests: a one-milestone plan, a plan with parallel
 * milestones, a plan of twelve, and tasks for a finished run and a run still in progress.
 * Shapes follow `gui/src/api/contract/`, not the real .orchestrate files.
 */

function dep(id: string, reason: string | null = null): PlanDependency {
  return { id, reason };
}

export function header(overrides: Partial<PlanHeader> = {}): PlanHeader {
  return {
    branch: "orch/desktop",
    base: "main",
    check_command: "cargo build --workspace && npm test",
    setup_command: null,
    isolation: "worktree",
    max_parallel: "2",
    scope: "warn",
    budget: "none",
    commits: "per-milestone",
    lead: "claude",
    lead_mode: "manual",
    workers: "rs-claude-1",
    reviewer: "rs-codex-1",
    risk_hold: "on",
    setup_inputs: null,
    extra: [],
    ...overrides,
  };
}

const criterion = (id: string, text: string): PlanCriterion => ({ id, text });

export function milestone(
  id: string,
  title: string,
  deps: PlanDependency[] = [],
  overrides: Partial<PlanMilestone> = {},
): PlanMilestone {
  return {
    id,
    title,
    enabled: true,
    goal: `Goal of ${id}.`,
    why: `Why ${id} exists.`,
    why_separate: "it is a distinct slice",
    depends_on: deps,
    prefer: null,
    tier: "medium",
    needs: ["coding", "tool-use"],
    model_why: "well specified and bounded",
    estimate: "60m",
    role: "front-end engineer",
    review: "yes",
    parallel: false,
    likely_files: [`gui/src/screens/${id.toLowerCase()}/`],
    acceptance_criteria: [criterion("AC1", `${id} works`), criterion("AC2", `${id} is tested`)],
    constraints: "stay in the listed files",
    interfaces: "uses the typed client",
    context: "M1 built the frame",
    out_of_scope: "editing the plan",
    open_questions: "none",
    has_open_questions: false,
    notes_from_user: "",
    ...overrides,
  };
}

/** One milestone, no dependencies: the smallest plan. */
export function oneMilestonePlan(): PlanResponse {
  return {
    title: "One milestone plan",
    header: header(),
    feature_breakdown: "1. a single slice",
    milestones: [milestone("M1", "Only one")],
  };
}

/** Three milestones where M2 and M3 run in parallel after M1. */
export function parallelPlan(): PlanResponse {
  return {
    title: "Parallel plan",
    header: header(),
    feature_breakdown: "1. the shared part\n2. two parallel slices",
    milestones: [
      milestone("M1", "Shared base"),
      milestone("M2", "Left side", [dep("M1", "needs the shared base")], { parallel: true }),
      milestone("M3", "Right side", [dep("M1", "needs the shared base")], { parallel: true }),
    ],
  };
}

/** Twelve milestones: a chain with two parallel branches off M2, meeting again at M11. */
export function twelvePlan(): PlanResponse {
  const m = (id: string, deps: string[] = [], why: string | null = null) =>
    milestone(id, `Milestone ${id} does its part`, deps.map((d) => dep(d, why)), {
      parallel: deps.length > 1 || id === "M4" || id === "M5",
    });
  return {
    title: "Twelve milestone plan",
    header: header(),
    feature_breakdown: "1. start\n2. branches\n3. the rest\n4. the end",
    milestones: [
      m("M1"),
      m("M2", ["M1"], "grows on the start"),
      m("M3", ["M2"], "the branches wait for it"),
      m("M4", ["M3"], "runs beside M5"),
      m("M5", ["M3"], "runs beside M4"),
      m("M6", ["M4"], "the branch moves on"),
      m("M7", ["M5"], "the branch moves on"),
      m("M8", ["M6"], "one after another"),
      m("M9", ["M7"], "one after another"),
      m("M10", ["M8", "M9"], "both branches meet"),
      m("M11", ["M10"], "the end is near"),
      m("M12", ["M11"], "the last step"),
    ],
  };
}

export function task(overrides: Partial<TaskInfo> & { id: string }): TaskInfo {
  return {
    title: `Milestone ${overrides.id} does its part`,
    state: "pending",
    reason: "",
    deps: [],
    worker: null,
    last_worker: null,
    model: null,
    tier: "medium",
    needs: ["coding"],
    ready: false,
    attempt: 1,
    fix_rounds: 0,
    review_rounds: 0,
    conflict_rounds: 0,
    lost: 0,
    escalated: false,
    ...overrides,
  };
}

/** A finished run over the parallel plan: M1 and M2 merged clean, M3 failed. */
export function finishedTasks(): TasksResponse {
  return {
    tasks: [
      task({ id: "M1", state: "merged", worker: "rs-claude-1", model: "claude-sonnet-5-5" }),
      task({ id: "M2", state: "merged", worker: "rs-codex-1", model: "codex-5", fix_rounds: 1 }),
      task({ id: "M3", state: "failed", worker: "rs-claude-1", model: "claude-sonnet-5-5", lost: 1 }),
    ],
  };
}

/** The same plan, still going: M1 merged, M2 verifying, M3 not started. */
export function runningTasks(): TasksResponse {
  return {
    tasks: [
      task({ id: "M1", state: "merged", worker: "rs-claude-1", model: "claude-sonnet-5-5" }),
      task({ id: "M2", state: "verifying", worker: "rs-codex-1", model: "codex-5", ready: false }),
      task({ id: "M3", state: "pending", ready: true }),
    ],
  };
}

const baseReport: ReportResponse = {
  branch: "orch/desktop",
  merged: 2,
  total: 3,
  failed: 1,
  conflicts: 0,
  lost: 1,
  fix_rounds: 1,
  broken_branch: null,
  started: 1_700_000_000,
  finished: 1_700_003_600,
  elapsed_s: 3600,
  first_pass: 1,
  first_pass_rate: 0.5,
  wasted: { lost_attempts: 1, fix_rounds: 1, conflict_rounds: 0, extra_attempts: 0 },
  tasks: [
    {
      id: "M1",
      state: "merged",
      tries: 1,
      fix_rounds: 0,
      review_rounds: 1,
      conflict_rounds: 0,
      lost: 0,
      planned_tier: "medium",
      model: "claude-sonnet-5-5",
      actual_tier: "medium",
      below_planned_tier: false,
      started: 1_700_000_000,
      finished: 1_700_001_200,
      seconds: 1200,
    },
    {
      id: "M2",
      state: "merged",
      tries: 2,
      fix_rounds: 1,
      review_rounds: 2,
      conflict_rounds: 0,
      lost: 0,
      planned_tier: "medium",
      model: "codex-5",
      actual_tier: "medium",
      below_planned_tier: false,
      started: 1_700_001_200,
      finished: 1_700_002_800,
      seconds: 1600,
    },
    {
      id: "M3",
      state: "failed",
      tries: 2,
      fix_rounds: 0,
      review_rounds: 0,
      conflict_rounds: 0,
      lost: 1,
      planned_tier: "medium",
      model: "claude-sonnet-5-5",
      actual_tier: "small",
      below_planned_tier: true,
      started: 1_700_002_800,
      finished: null,
      seconds: null,
    },
  ],
  models: [
    {
      model: "claude-sonnet-5-5",
      tier: "medium",
      tasks: 2,
      merged: 1,
      tries: 3,
      fix_rounds: 0,
      review_rounds: 1,
      conflict_rounds: 0,
      lost: 1,
    },
    {
      model: "codex-5",
      tier: "medium",
      tasks: 1,
      merged: 1,
      tries: 2,
      fix_rounds: 1,
      review_rounds: 2,
      conflict_rounds: 0,
      lost: 0,
    },
  ],
};

/** A finished run: every milestone is in a final state and `finished` is set. */
export function finishedReport(): ReportResponse {
  return baseReport;
}

/** A run in progress: one milestone still active, no finish time. */
export function runningReport(): ReportResponse {
  return {
    ...baseReport,
    merged: 1,
    failed: 0,
    total: 3,
    finished: null,
    elapsed_s: null,
    first_pass: 1,
    first_pass_rate: 1,
    wasted: { lost_attempts: 0, fix_rounds: 0, conflict_rounds: 0, extra_attempts: 0 },
    tasks: [
      baseReport.tasks[0]!,
      { ...baseReport.tasks[1]!, state: "verifying" },
      { ...baseReport.tasks[2]!, state: "verifying", lost: 0, tries: 1, started: 1_700_002_800, finished: null, seconds: null },
    ],
    models: [
      { ...baseReport.models[0]!, tasks: 2, merged: 1, lost: 0 },
      baseReport.models[1]!,
    ],
  };
}
