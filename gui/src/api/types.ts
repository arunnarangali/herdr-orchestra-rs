// Request and response shapes of `orch serve` (`/api/v1`), written to match `crates/orch-api`.
// `contract/` holds JSON the Rust tests produce from the real shapes; `contract.ts` makes the
// typecheck fail when any of it stops fitting these types. States and kinds are strings in
// their CLI spellings; `states.ts` names the ones the app branches on.

export interface Health {
  status: string;
  version: string;
  project: string;
  run: boolean;
}

export interface ErrorDetail {
  code: string;
  message: string;
  request_id: string;
}
export interface ErrorBody {
  error: ErrorDetail;
}

// ------------------------------------------------------------------------------- status

export interface BrokenInfo {
  reason: string;
  since: number;
}
export interface EventManagerInfo {
  running: boolean;
  holder: string | null;
}
export interface EventCounts {
  unread: number;
  open: number;
}
export interface StateCount {
  state: string;
  count: number;
}
export interface WorkerInfo {
  name: string;
  kind: string;
  model: string | null;
  tier: string | null;
  detect: boolean;
  task: string | null;
}
export interface TaskBrief {
  id: string;
  title: string;
  state: string;
  deps: string[];
  worker: string | null;
  tier: string | null;
  reason: string;
}
export interface StatusResponse {
  title: string;
  branch: string;
  base: string | null;
  isolation: string;
  max_parallel: number;
  reviewer: string | null;
  lead_mode: string;
  /** `running`, `idle`, `all_done`, `stopped` or `broken_branch`. */
  run_state: string;
  all_done: boolean;
  stuck: boolean;
  broken_branch: BrokenInfo | null;
  event_manager: EventManagerInfo;
  events: EventCounts;
  progress: StateCount[];
  workers: WorkerInfo[];
  tasks: TaskBrief[];
}

// ------------------------------------------------------------------------------- plan

export interface PlanHeader {
  branch: string | null;
  base: string | null;
  check_command: string | null;
  setup_command: string | null;
  isolation: string | null;
  max_parallel: string | null;
  scope: string | null;
  budget: string | null;
  commits: string | null;
  lead: string | null;
  lead_mode: string | null;
  workers: string | null;
  reviewer: string | null;
  risk_hold: string | null;
  setup_inputs: string | null;
  /** Other header lines as `[name, value]` pairs. */
  extra: string[][];
}
export interface PlanDependency {
  id: string;
  reason: string | null;
}
export interface PlanCriterion {
  id: string;
  text: string;
}
export interface PlanMilestone {
  id: string;
  title: string;
  enabled: boolean;
  goal: string;
  why: string;
  why_separate: string | null;
  depends_on: PlanDependency[];
  prefer: string | null;
  tier: string | null;
  needs: string[];
  model_why: string | null;
  estimate: string | null;
  role: string | null;
  review: string | null;
  parallel: boolean;
  likely_files: string[];
  acceptance_criteria: PlanCriterion[];
  constraints: string;
  interfaces: string;
  context: string;
  out_of_scope: string;
  open_questions: string;
  has_open_questions: boolean;
  notes_from_user: string;
}
export interface PlanResponse {
  title: string | null;
  header: PlanHeader;
  feature_breakdown: string;
  milestones: PlanMilestone[];
}

// ------------------------------------------------------------------------------- tasks

export interface TaskInfo {
  id: string;
  title: string;
  state: string;
  reason: string;
  deps: string[];
  worker: string | null;
  last_worker: string | null;
  model: string | null;
  tier: string | null;
  needs: string[];
  ready: boolean;
  attempt: number;
  fix_rounds: number;
  review_rounds: number;
  conflict_rounds: number;
  lost: number;
  escalated: boolean;
}
export interface TasksResponse {
  tasks: TaskInfo[];
}
export interface HistoryEntry {
  ts: number;
  from: string;
  to: string;
  why: string;
  worker: string | null;
  model: string | null;
  attempt: number;
  forced: boolean;
}
export interface CheckInfo {
  /** `worktree` or `branch`. */
  where: string;
  ok: boolean;
  secs: number;
  ts: number;
}
export interface Finding {
  file: string;
  line: string | null;
  problem: string;
  why: string;
  criterion: string | null;
}
export interface CriterionVerdict {
  id: string;
  met: boolean;
  note: string;
}
export interface ReviewInfo {
  round: number;
  error: string | null;
  outcome: string | null;
  criteria: CriterionVerdict[];
  blockers: Finding[];
  suggestions: Finding[];
}
export interface ResultCheck {
  ran: boolean;
  passed: boolean;
  summary: string;
}
export interface ResultCriterion {
  id: string;
  met: boolean;
  evidence: string;
}
export interface CoderResult {
  attempt: number;
  error: string | null;
  status: string | null;
  commit: string | null;
  files_changed: string[];
  check: ResultCheck | null;
  criteria: ResultCriterion[];
  questions: string[];
}
export interface TaskDetailResponse {
  task: TaskInfo;
  history: HistoryEntry[];
  checks: CheckInfo[];
  reviews: ReviewInfo[];
  results: CoderResult[];
  files_changed: string[];
  files_outside_likely: string[];
}

// ------------------------------------------------------------------------------- events

export interface EventInfo {
  id: number;
  ts: number;
  kind: string;
  task: string | null;
  task_title: string | null;
  text: string;
  agent?: string | null;
  prompt?: string | null;
  read: boolean;
  open: boolean;
  help: string;
}
export interface ReadyInfo {
  task: string;
  title: string;
  tier: string | null;
  workers: string[];
}
export interface EventsResponse {
  lead_on: boolean;
  new: EventInfo[];
  still_open: EventInfo[];
  ready: ReadyInfo[];
}

// ------------------------------------------------------------------------------- report

export interface ReportRowInfo {
  id: string;
  state: string;
  tries: number;
  fix_rounds: number;
  review_rounds: number;
  conflict_rounds: number;
  lost: number;
  planned_tier: string | null;
  model: string | null;
  actual_tier: string | null;
  below_planned_tier: boolean;
  started: number | null;
  finished: number | null;
  seconds: number | null;
}
export interface ModelUse {
  model: string;
  tier: string | null;
  tasks: number;
  merged: number;
  tries: number;
  fix_rounds: number;
  review_rounds: number;
  conflict_rounds: number;
  lost: number;
}
export interface WastedWork {
  lost_attempts: number;
  fix_rounds: number;
  conflict_rounds: number;
  extra_attempts: number;
}
export interface ReportResponse {
  branch: string;
  merged: number;
  total: number;
  failed: number;
  conflicts: number;
  lost: number;
  fix_rounds: number;
  broken_branch: BrokenInfo | null;
  started: number | null;
  finished: number | null;
  elapsed_s: number | null;
  first_pass: number;
  first_pass_rate: number | null;
  wasted: WastedWork;
  tasks: ReportRowInfo[];
  models: ModelUse[];
}

// ------------------------------------------------------------------------------- agents, models, logs

export interface LoginInfo {
  /** `authenticated`, `not authenticated` or `unknown`. */
  state: string;
  reason: string;
}
export interface SkillsInfo {
  /** `current`, `outdated`, `missing` or `none`. */
  status: string;
  problems: string[];
}
export interface AgentKindInfo {
  kind: string;
  display_name: string | null;
  installed: boolean;
  roles: string[];
  herdr_detect: boolean;
  verified: boolean;
  launch: string;
  skills: SkillsInfo;
  login: LoginInfo;
}
export interface TeamMember {
  name: string;
  /** `worker`, `reviewer` or `lead`. */
  role: string;
  kind: string;
  model: string | null;
  tier: string | null;
  detect: boolean;
}
export interface AgentsResponse {
  agents: AgentKindInfo[];
  team: TeamMember[];
}
export interface ModelInfo {
  id: string;
  tier: string | null;
  kinds: string[];
  reasoning: number | null;
  coding: number | null;
  architecture: number | null;
  instructions: number | null;
  tools: number | null;
  planning: number | null;
  speed: number | null;
  context_k: number | null;
  open_weights: boolean | null;
  evidence: string | null;
  denied: boolean;
  denied_by: string | null;
}
export interface TierInfo {
  name: string;
  reasoning_floor: number;
  purpose: string | null;
}
export interface PolicyInfo {
  deny: string[];
  deny_why: string | null;
}
export interface ModelsResponse {
  models: ModelInfo[];
  tiers: TierInfo[];
  policy: PolicyInfo;
}
export interface LogsResponse {
  lines: string[];
}

/** The OpenAPI 3 document: typed loosely, the app only reads its version and paths. */
export interface OpenApiDocument {
  openapi: string;
  info: { title: string; version: string };
  paths: Record<string, unknown>;
  components?: unknown;
}

// ------------------------------------------------------------------------------- actions

export interface AnswerBody {
  text: string;
}
export interface DispatchBody {
  ids: string[];
  to?: string | null;
  /** A minimum tier: `small`, `medium` or `high`. */
  tier?: string | null;
}
export interface FixBody {
  text: string;
  to?: string | null;
}
export interface EscalateBody {
  reason?: string | null;
}
export interface SetStatusBody {
  /** `pending`, `paused`, `failed` or `skipped`. */
  status: string;
  reason?: string | null;
}
export interface TaskAfter {
  id: string;
  title: string;
  state: string;
  worker: string | null;
}
export interface ActionResponse {
  action: string;
  tasks: TaskAfter[];
  event_manager_running: boolean;
  /** Recorded, but no event manager is running to carry it out. */
  waiting: boolean;
  message: string;
}
export interface RunControlResponse {
  action: string;
  /** `running`, `paused`, `stopping` or `stopped`. */
  run_control: string;
  event_manager_running: boolean;
  waiting: boolean;
  message: string;
}

// ------------------------------------------------------------------------------- stream

/** Payload of a `task-state` message. */
export interface TaskStateEvent {
  id: string;
  task: string;
  task_id?: string;
  from: string;
  to: string;
  status: string;
  why: string;
  reason?: string;
  worker?: string;
  agent?: string;
  model?: string;
  attempt: number;
  forced: boolean;
  ts: number;
}
/** Payload of a `lead-event` message. */
export interface LeadQueueEvent {
  id: number;
  ts: number;
  kind: string;
  task?: string;
  task_id?: string;
  text: string;
  agent?: string;
  prompt?: string;
  read_at?: number;
}
/** Payload of a `run-state` message (also sent as a "re-fetch" hint after a gap). */
export interface RunStateEvent {
  action?: string;
  message?: string;
  event?: string;
  task?: string;
  agent?: string;
  status?: string;
  run_control?: string;
  state?: string;
  source?: string;
  ts?: number;
}
/** Payload of a `log` message. */
export interface LogEvent {
  message: string;
  line: string;
  ts: number;
}

/**
 * The server's own record of a message. On the wire `id` and `event` are SSE fields and
 * `data` is the JSON payload; see `StreamMessage` for what the app receives.
 */
export interface StreamMessageEnvelope {
  id: number;
  event: string;
  data: unknown;
}

export interface StreamPayloads {
  "task-state": TaskStateEvent;
  "lead-event": LeadQueueEvent;
  "run-state": RunStateEvent;
  log: LogEvent;
}
export type StreamEventName = keyof StreamPayloads;
export const STREAM_EVENT_NAMES: readonly StreamEventName[] = [
  "task-state",
  "lead-event",
  "run-state",
  "log",
];

/** One message from the live stream, typed by its event name. */
export type StreamMessage = {
  [K in StreamEventName]: { id: string; event: K; data: StreamPayloads[K] };
}[StreamEventName];
