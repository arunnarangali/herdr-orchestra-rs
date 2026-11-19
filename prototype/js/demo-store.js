/* Herdr Orchestra — Demo Data Layer.
   A single in-memory store whose snapshots match the /api/v1/ REST + SSE contract shapes:
   status, plan, tasks, events, attention, chat, agents, models, report.
   Exposed as window.HerdrStore (classic script so pages also work from file://). */
(function (global) {
  'use strict';

  const API_BASE = '/api/v1';
  const ANSI_RE = /\u001b\[[0-9;?]*[ -/]*[@-~]/g;
  const stripAnsi = (s) => String(s).replace(ANSI_RE, '');
  const clone = (v) => structuredClone(v);
  const nowIso = () => new Date().toISOString();
  const todayAt = (h, m, sec = 0) => { const d = new Date(); d.setHours(h, m, sec, 0); return d.toISOString(); };
  const clk = (h, m, sec) => h * 3600 + m * 60 + sec;
  const fmtClock = (secs) => {
    const p = (n) => String(n).padStart(2, '0');
    const hh = Math.floor(secs / 3600) % 24;
    return `${p(hh % 12 || 12)}:${p(Math.floor((secs % 3600) / 60))}:${p(secs % 60)}`;
  };

  function fmtTime(iso) {
    const d = new Date(iso);
    const t = d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
    if (d.toDateString() === new Date().toDateString()) return `Today, ${t}`;
    return `${d.toLocaleDateString([], { month: 'short', day: 'numeric' })}, ${t}`;
  }

  let uid = 100;
  const nextId = (p) => `${p}-${++uid}`;

  /* ------------------------------------------------------------------ seed data */

  const PROJECTS = [
    { id: 'herdr-orchestra', name: 'herdr-orchestra', path: '/mnt/workspace/projects/herdr-orchestra-rs' },
    { id: 'streamforge', name: 'streamforge', path: '/mnt/workspace/projects/streamforge' },
  ];

  const MODELS = {
    tiers: [
      { tier: 'small', agent: 'claude', model: 'haiku', note: 'Mechanical edits, docs, renames' },
      { tier: 'medium', agent: 'claude', model: 'sonnet', note: 'Feature work with clear scope' },
      { tier: 'high', agent: 'claude', model: 'opus', note: 'Schema, API design, risky changes' },
    ],
    available: [
      { agent: 'claude', model: 'opus', status: 'ready' },
      { agent: 'claude', model: 'sonnet', status: 'ready' },
      { agent: 'claude', model: 'haiku', status: 'ready' },
      { agent: 'codex', model: 'gpt-6-sol', status: 'ready' },
    ],
  };

  function baseHerdr() {
    return {
      status: {
        project: 'herdr-orchestra',
        branch: 'feature/categories',
        herdr: { connected: true, version: '0.9.2', socket: '/run/user/1000/herdr.sock' },
        driver: { kind: 'autopilot', command: 'orch autopilot', pid: 4821, state: 'active' },
        lead: { name: 'lead', agent: 'claude', model: 'opus', state: 'waiting_for_go', typing: false },
      },
      plan: {
        id: 'plan-categories',
        title: 'Categories Feature',
        isolation: 'worktree',
        max_parallel: 2,
        approved: false,
        revision: 1,
        milestones: [
          { id: 'M1', title: 'Design DB schema and migration', tier: 'medium', depends_on: [], branch: 'orch/m1-schema' },
          { id: 'M2', title: 'Implement categories REST API (CRUD)', tier: 'high', depends_on: [], branch: 'orch/m2-categories-api' },
          { id: 'M3', title: 'Build React categories page (list, add, edit, delete)', tier: 'medium', depends_on: ['M2'], branch: 'orch/m3-categories-ui' },
          { id: 'M4', title: 'Add seed data, tests and docs', tier: 'high', depends_on: ['M1', 'M2', 'M3'], branch: 'orch/m4-seed-tests-docs' },
        ],
      },
      tasks: [
        { milestone: 'M1', state: 'review', agent: 'reviewer', branch: 'orch/m1-schema', attempts: 1 },
        { milestone: 'M2', state: 'coding', agent: 'coder-1', branch: 'orch/m2-categories-api', attempts: 1 },
        { milestone: 'M3', state: 'pending', agent: null, branch: 'orch/m3-categories-ui', attempts: 0 },
        { milestone: 'M4', state: 'held', agent: null, branch: 'orch/m4-seed-tests-docs', attempts: 0 },
      ],
      events: [
        { seq: 1, ts: todayAt(23, 24), type: 'plan.created', milestone: null, message: 'Plan "Categories Feature" created with 4 milestones' },
        { seq: 2, ts: todayAt(23, 25), type: 'milestone.started', milestone: 'M1', message: 'M1 started by coder-2' },
        { seq: 3, ts: todayAt(23, 27), type: 'milestone.started', milestone: 'M2', message: 'M2 started by coder-1' },
        { seq: 4, ts: todayAt(23, 29), type: 'milestone.review', milestone: 'M1', message: 'M1 sent to reviewer' },
        { seq: 5, ts: todayAt(23, 30), type: 'milestone.held', milestone: 'M4', message: 'M4 held: touches migration file' },
        { seq: 6, ts: todayAt(23, 31), type: 'question.asked', milestone: 'M2', message: 'coder-1 asked: soft-delete or hard-delete?' },
      ],
      attention: [
        {
          id: 'att-m4-hold', kind: 'hold', milestone: 'M4', agent: null,
          title: 'M4 Held — Risk Hold: touches migration file',
          body: 'The milestone M4 is held for approval because it modifies a migration file (schema change).',
          options: [{ id: 'approve', label: 'Approve M4', icon: 'check', tone: 'amber' }],
          status: 'open', created_at: todayAt(23, 30), resolution: null,
        },
        {
          id: 'att-m2-q1', kind: 'question', milestone: 'M2', agent: 'coder-1',
          title: 'Question from coder-1 (M2)',
          body: 'Soft-delete or hard-delete categories?',
          options: [
            { id: 'soft', label: 'Hide (soft-delete)', tone: 'cyan' },
            { id: 'hard', label: 'Remove', icon: 'trash', tone: 'neutral' },
          ],
          status: 'open', created_at: todayAt(23, 31), resolution: null,
        },
      ],
      chat: [
        { id: 'msg-1', role: 'user', author: 'Arun N', ts: todayAt(23, 24), text: 'Add categories endpoint and page.' },
        {
          id: 'msg-2', role: 'lead', author: 'lead', ts: todayAt(23, 24),
          text: "Got it. I've created an execution plan for the categories feature with 4 milestones. The plan uses a worktree for isolation and will run with a max parallel of 2 agents.",
          attachment: { type: 'plan', plan_id: 'plan-categories' },
        },
      ],
      agents: [
        {
          id: 'coder-1', role: 'coder', cli: 'claude', model: 'sonnet', pane: '1-2', state: 'coding', milestone: 'M2',
          task: 'Implement categories API', detail: 'Working on M2 · Implement categories API',
          output: [
            '\u001b[36m●\u001b[0m Reading crates/api/src/routes/mod.rs',
            '\u001b[32m+\u001b[0m Added route GET /api/v1/categories',
            '\u001b[32m+\u001b[0m Added route POST /api/v1/categories',
            '\u001b[33m?\u001b[0m Asking lead: soft-delete or hard-delete categories?',
            '\u001b[2m  waiting on answer — continuing with list + create\u001b[0m',
          ],
        },
        {
          id: 'coder-2', role: 'coder', cli: 'codex', model: 'gpt-6-sol', pane: '1-3', state: 'idle', milestone: null,
          task: null, detail: 'No active task · Awaiting assignment',
          output: ['\u001b[32m✓\u001b[0m M1 handed to reviewer', '\u001b[2m  idle\u001b[0m'],
        },
        {
          id: 'reviewer', role: 'reviewer', cli: 'claude', model: 'opus', pane: '1-4', state: 'reviewing', milestone: 'M1',
          task: 'Schema and migration', detail: 'Reviewing M1 · Schema and migration',
          output: [
            '\u001b[36m●\u001b[0m git diff main...orch/m1-schema',
            '\u001b[32m+\u001b[0m migrations/0007_create_categories.sql (24 lines)',
            '\u001b[33m!\u001b[0m index on categories(slug) should be UNIQUE',
            '\u001b[2m  drafting review comments\u001b[0m',
          ],
        },
      ],
      report: {
        run_id: 'run-categories-001', started_at: todayAt(23, 24), duration_s: 1260,
        cost: { usd: 3.42, tokens_in: 412880, tokens_out: 38214 },
      },
    };
  }

  function baseStreamforge() {
    return {
      status: {
        project: 'streamforge',
        branch: 'feature/backpressure',
        herdr: { connected: true, version: '0.9.2', socket: '/run/user/1000/herdr.sock' },
        driver: { kind: 'autopilot', command: 'orch autopilot', pid: 5110, state: 'active' },
        lead: { name: 'lead', agent: 'claude', model: 'opus', state: 'running', typing: false },
      },
      plan: {
        id: 'plan-backpressure',
        title: 'Ingest Backpressure',
        isolation: 'worktree',
        max_parallel: 2,
        approved: true,
        revision: 1,
        milestones: [
          { id: 'M1', title: 'Add bounded channel to ingest pipeline', tier: 'medium', depends_on: [], branch: 'orch/m1-bounded-channel' },
          { id: 'M2', title: 'Surface backpressure metrics', tier: 'medium', depends_on: ['M1'], branch: 'orch/m2-metrics' },
          { id: 'M3', title: 'Load test and tune limits', tier: 'high', depends_on: ['M2'], branch: 'orch/m3-load-test' },
        ],
      },
      tasks: [
        { milestone: 'M1', state: 'merged', agent: 'coder-1', branch: 'orch/m1-bounded-channel', attempts: 1 },
        { milestone: 'M2', state: 'failed', agent: 'coder-1', branch: 'orch/m2-metrics', attempts: 2 },
        { milestone: 'M3', state: 'pending', agent: null, branch: 'orch/m3-load-test', attempts: 0 },
      ],
      events: [
        { seq: 1, ts: todayAt(21, 2), type: 'plan.approved', milestone: null, message: 'Plan "Ingest Backpressure" approved' },
        { seq: 2, ts: todayAt(21, 40), type: 'milestone.merged', milestone: 'M1', message: 'M1 merged into feature/backpressure' },
        { seq: 3, ts: todayAt(22, 15), type: 'milestone.failed', milestone: 'M2', message: 'M2 failed: 3 tests failing in metrics::tests' },
      ],
      attention: [
        {
          id: 'att-sf-m2-fail', kind: 'failure', milestone: 'M2', agent: 'coder-1',
          title: 'M2 Failed — 3 tests failing',
          body: 'metrics::tests::gauge_resets, metrics::tests::histogram_buckets and 1 more failed twice.',
          options: [
            { id: 'retry', label: 'Retry M2', icon: 'play', tone: 'cyan' },
            { id: 'dismiss', label: 'Dismiss', tone: 'neutral' },
          ],
          status: 'open', created_at: todayAt(22, 15), resolution: null,
        },
      ],
      chat: [
        { id: 'sf-1', role: 'user', author: 'Arun N', ts: todayAt(21, 0), text: 'Add backpressure to the ingest pipeline.' },
        { id: 'sf-2', role: 'lead', author: 'lead', ts: todayAt(21, 1), text: 'Planned 3 milestones. Starting once you say go.', attachment: { type: 'plan', plan_id: 'plan-backpressure' } },
        { id: 'sf-3', role: 'lead', author: 'lead', ts: todayAt(22, 16), text: 'M2 failed twice on metrics tests. I need a decision before retrying.' },
      ],
      agents: [
        {
          id: 'coder-1', role: 'coder', cli: 'claude', model: 'sonnet', pane: '2-2', state: 'idle', milestone: null,
          task: null, detail: 'No active task · Awaiting assignment',
          output: ['\u001b[31m✗\u001b[0m cargo test -p metrics: 3 failed', '\u001b[2m  idle\u001b[0m'],
        },
      ],
      report: {
        run_id: 'run-backpressure-001', started_at: todayAt(21, 2), duration_s: 4980,
        cost: { usd: 7.9, tokens_in: 902114, tokens_out: 81902 },
      },
    };
  }


  /* ------------------------------------------------------------------ workflow (screen 02) + run (screen 03) seeds */

  function ms(id, title, tier, state, agent, depends_on, why, goal, files, criteria, interfaces) {
    return {
      id, title, tier, state, agent, depends_on, why, goal, needs: ['coding', 'tool-use'], files,
      criteria: criteria.map(([text, done], i) => ({ id: `AC${i + 1}`, text, done })),
      interfaces, included: true,
    };
  }

  function workflowCategories() {
    return {
      title: 'Categories Endpoint & UI', base: 'main', branch: 'feature/categories',
      check: 'cargo test && npm test', isolation: 'worktree', scope: 'warn', risk_hold: true, commits: 'per-milestone',
      synced_at: todayAt(23, 18),
      milestones: [
        ms('M1', 'Category DB Schema & API Contract', 'high', 'merged', null, [],
          'Foundation & stable API contract for parallel work.',
          'Define the categories table, its migration and the API contract that M2 and M3 build on.',
          ['migrations/0007_create_categories.sql', 'src/api/contract.rs'],
          [['The categories table and migration apply cleanly on an empty database.', true], ['The API contract (request and response schema) is documented for M2 and M3.', true]],
          ['-- migrations/0007_create_categories.sql', 'CREATE TABLE categories (', '    id UUID PRIMARY KEY,', '    name TEXT NOT NULL UNIQUE,', '    created_at TIMESTAMPTZ NOT NULL DEFAULT now()', ');']),
        ms('M2', 'Categories REST Endpoints', 'medium', 'coding', 'coder-1', ['M1'],
          'Independent backend endpoint after M1 contract.',
          'Implement REST API endpoints for category CRUD operations.',
          ['src/api/categories.rs', 'tests/categories_test.rs'],
          [['CRUD endpoints for categories (POST, GET, PUT, DELETE) work and pass tests.', true], ['Validation, error handling and contract (response schema) match M1.', false]],
          ['pub trait CategoryService {', '    async fn create(req: CreateCategory) -> Result<Category>;', '    async fn list(q: CategoryQuery) -> Result<Vec<Category>>;', '    async fn update(id: Uuid, req: UpdateCategory) -> Result<()>;', '    async fn delete(id: Uuid) -> Result<()>;', '}']),
        ms('M3', 'Categories Frontend Page', 'medium', 'coding', 'coder-2', ['M1'],
          'Separate UI work that can run in parallel with M2.',
          'Build the React categories page with list, add, edit and delete.',
          ['web/src/pages/Categories.tsx', 'web/src/api/categories.ts'],
          [['The page lists categories and supports add, edit and delete.', false], ['Loading, empty and error states are handled.', false]],
          ['GET    /api/v1/categories', 'POST   /api/v1/categories', 'PUT    /api/v1/categories/:id', 'DELETE /api/v1/categories/:id']),
        ms('M4', 'Navigation & Integration Wiring', 'small', 'pending', null, ['M2', 'M3'],
          'Final integration and navigation after M2 + M3.',
          'Wire the categories page into navigation and verify the full flow.',
          ['web/src/routes.tsx', 'src/routes.rs'],
          [['Categories appears in the main navigation.', false], ['The end-to-end flow works with seed data.', false]],
          ['// no new interfaces: wires M2 and M3 together']),
      ],
    };
  }

  function runCategories() {
    const L = (ts, text) => ({ ts, text });
    return {
      id: 8421, state: 'live', plan: 'Categories Endpoint & UI', branch: 'feature/categories',
      started_at: new Date(2026, 10, 17, 23, 18).toISOString(),
      percent: 65, merged: 2, total: 4, coding: 2,
      elapsed_s: 14 * 60 + 22, budget_s: 3 * 3600, clock_s: clk(23, 24, 51), ticks: 0, rot: 0,
      agents: [
        {
          id: 'coder-1', cli: 'claude', model: 'sonnet', icon: 'code', state: 'coding', badge: 'CODING (Attempt 1)', pane: '1-2',
          rows: [
            { label: 'Task', kind: 'text', value: 'M2: Categories REST Endpoints' },
            { label: 'Worktree', kind: 'copy', value: '../repo-wt/coder-1' },
            { label: 'Check', kind: 'check', value: 'cargo check: passing', ok: true },
          ],
        },
        {
          id: 'coder-2', cli: 'codex', model: 'gpt-6-sol', icon: 'code', state: 'verifying', badge: 'VERIFYING', pane: '1-3',
          rows: [
            { label: 'Task', kind: 'text', value: 'M3: Categories Frontend Page' },
            {
              label: 'Step', kind: 'steps', steps: [
                { label: 'Contract', state: 'done' }, { label: 'Code', state: 'done' }, { label: 'Check Running', state: 'active' },
                { label: 'Review', state: 'pending' }, { label: 'Merge', state: 'pending' },
              ],
            },
          ],
        },
        {
          id: 'reviewer', cli: 'claude', model: 'opus', icon: 'shield', state: 'approved', badge: 'APPROVED', pane: '1-4',
          rows: [
            { label: 'Task', kind: 'text', value: 'M1: Schema & Contract' },
            { label: 'Result', kind: 'check', value: 'Approved', ok: true },
            { label: 'Notes', kind: 'text', value: '0 blockers, 1 suggestion' },
          ],
        },
      ],
      tabs: [
        { id: 'coder-1', label: 'coder-1 output', pane: '1-2' },
        { id: 'coder-2', label: 'coder-2 output', pane: '1-3' },
        { id: 'autopilot', label: 'autopilot.log', pane: '1-1' },
      ],
      outputs: {
        'coder-1': [
          L('11:20:08', '> coder-1 (claude:sonnet) — M2: Categories REST Endpoints'),
          L('11:20:09', 'Planning changes and loading M1 contract...'),
          L('11:20:16', '> Creating branch: feature/categories-api (worktree: ../repo-wt/coder-1)'),
          L('11:20:21', '> Scaffolding endpoint: GET/POST/PUT/DELETE /api/categories'),
          L('11:21:04', '> Editing files:'),
          L(null, '  - src/api/categories.rs   (new)'),
          L(null, '  - src/routes.rs           (updated)'),
          L(null, '  - src/models/category.rs  (updated)'),
          L('11:23:11', '> Writing tests: tests/categories_test.rs'),
          L('11:24:37', 'Running checks...'),
          L('11:24:37', '$ cargo check'),
          L('11:24:41', '\u001b[32m✓\u001b[0m cargo check: passing (0 errors, 0 warnings)'),
          L('11:24:44', 'Running tests...'),
          L('11:24:44', '$ cargo test'),
          L('11:24:51', '\u001b[32m✓\u001b[0m 6 tests passed (0 failed, 0 ignored)'),
          L('11:24:51', 'Task status: coding (Attempt 1)'),
          L('11:24:51', 'Next: finalize changes and handoff for review.'),
        ],
        'coder-2': [
          L('11:19:24', '> coder-2 (codex:gpt-6-sol) — M3: Categories Frontend Page'),
          L('11:19:25', 'Loading M1 contract and web/src/api client...'),
          L('11:19:40', '> Creating branch: feature/categories-ui (worktree: ../repo-wt/coder-2)'),
          L('11:20:30', '> Editing files:'),
          L(null, '  - web/src/pages/Categories.tsx   (new)'),
          L(null, '  - web/src/api/categories.ts      (new)'),
          L('11:23:02', '$ npm run typecheck'),
          L('11:23:20', '\u001b[32m✓\u001b[0m typecheck: passing'),
          L('11:23:41', '$ npm test -- Categories'),
        ],
        autopilot: [
          L('11:18:02', '[autopilot] run #8421 started (plan: Categories Endpoint & UI)'),
          L('11:18:04', '[autopilot] spawned coder-1 (claude:sonnet), coder-2 (codex:gpt-6-sol), reviewer (claude:opus)'),
          L('11:19:02', '[autopilot] M1 verification passed'),
          L('11:19:18', '[autopilot] M1 merged into feature/categories'),
          L('11:19:21', '[autopilot] M2, M3 ready; dispatching in parallel (max 2)'),
          L('11:20:03', '[autopilot] lead woken: READY'),
          L('11:23:56', '[autopilot] QuestionAsked M2 — waiting on lead'),
        ],
      },
      script: {
        'coder-1': [
          '> Committing: feat(categories): add CRUD endpoints',
          'Waiting on answer: soft-delete or hard-delete? Continuing with list + create.',
          '\u001b[32m✓\u001b[0m cargo clippy: no warnings',
        ],
        'coder-2': [
          '\u001b[32m✓\u001b[0m 4 tests passed (0 failed)',
          'Task status: verifying (Check Running)',
          'Next: request review for M3.',
        ],
        autopilot: [
          '[autopilot] tick: M2 coding, M3 verifying, reviewer idle',
          '[autopilot] waiting on lead answer for M2',
        ],
      },
    };
  }

  function workflowStreamforge() {
    return {
      title: 'Ingest Backpressure', base: 'main', branch: 'feature/backpressure',
      check: 'cargo test --workspace', isolation: 'worktree', scope: 'warn', risk_hold: false, commits: 'per-milestone',
      synced_at: todayAt(21, 2),
      milestones: [
        ms('M1', 'Bounded Channel in Ingest Pipeline', 'medium', 'merged', null, [],
          'Foundation for backpressure; everything else reads its limits.',
          'Replace the unbounded ingest channel with a bounded one that applies backpressure.',
          ['crates/ingest/src/pipeline.rs'],
          [['Producers block when the channel is full.', true], ['Capacity is configurable.', true]],
          ['pub struct IngestChannel<T> {', '    pub fn bounded(capacity: usize) -> Self;', '}']),
        ms('M2', 'Backpressure Metrics', 'medium', 'failed', 'coder-1', ['M1'],
          'Observability can be built once the channel exists.',
          'Expose gauges and histograms that show queue depth and stall time.',
          ['crates/metrics/src/lib.rs', 'crates/metrics/tests/gauges.rs'],
          [['Queue depth is exported as a gauge.', true], ['Stall time is exported as a histogram.', false]],
          ['pub fn record_depth(depth: usize);', 'pub fn record_stall(ms: u64);']),
        ms('M3', 'Load Test and Tune Limits', 'high', 'pending', null, ['M2'],
          'Needs real metrics to tune against.',
          'Run a load test and choose default channel limits.',
          ['bench/ingest_load.rs'],
          [['A load test reproduces the stall scenario.', false], ['Default limits are documented.', false]],
          ['// no new interfaces']),
      ],
    };
  }

  function runStreamforge() {
    const L = (ts, text) => ({ ts, text });
    return {
      id: 8377, state: 'live', plan: 'Ingest Backpressure', branch: 'feature/backpressure',
      started_at: new Date(2026, 10, 17, 21, 2).toISOString(),
      percent: 40, merged: 1, total: 3, coding: 0,
      elapsed_s: 83 * 60 + 5, budget_s: 4 * 3600, clock_s: clk(22, 25, 0), ticks: 0, rot: 0,
      agents: [
        {
          id: 'coder-1', cli: 'claude', model: 'sonnet', icon: 'code', state: 'failed', badge: 'FAILED (Attempt 2)', pane: '2-2',
          rows: [
            { label: 'Task', kind: 'text', value: 'M2: Backpressure Metrics' },
            { label: 'Worktree', kind: 'copy', value: '../streamforge-wt/coder-1' },
            { label: 'Check', kind: 'check', value: 'cargo test: 3 failing', ok: false },
          ],
        },
      ],
      tabs: [
        { id: 'coder-1', label: 'coder-1 output', pane: '2-2' },
        { id: 'autopilot', label: 'autopilot.log', pane: '2-1' },
      ],
      outputs: {
        'coder-1': [
          L('10:12:40', '> coder-1 (claude:sonnet) — M2: Backpressure Metrics'),
          L('10:14:02', '$ cargo test -p metrics'),
          L('10:14:30', '\u001b[31m✗\u001b[0m metrics::tests::gauge_resets failed'),
          L('10:14:30', '\u001b[31m✗\u001b[0m metrics::tests::histogram_buckets failed'),
          L('10:14:31', 'Task status: failed (Attempt 2)'),
        ],
        autopilot: [
          L('09:02:11', '[autopilot] run #8377 started (plan: Ingest Backpressure)'),
          L('09:40:02', '[autopilot] M1 merged into feature/backpressure'),
          L('10:15:00', '[autopilot] M2 failed twice; waiting on lead'),
        ],
      },
      script: {},
    };
  }

  function seedHerdrOrchestra() {
    const d = baseHerdr();
    d.workflow = workflowCategories();
    d.run = runCategories();
    const ev = (seq, h, m, sec, type, milestones, message, extra = {}) =>
      ({ seq, ts: todayAt(h, m, sec), type, milestone: milestones[0] || null, milestones, message, acked: false, ...extra });
    d.events = [
      ev(1, 23, 19, 2, 'VerificationPassed', ['M1'], 'M1 tests and contract verified.'),
      ev(2, 23, 19, 18, 'MergeCompleted', ['M1'], 'M1 merged into feature/categories.'),
      ev(3, 23, 19, 21, 'TaskReady', ['M2', 'M3'], 'Spawned parallel tasks for M2 and M3.'),
      ev(4, 23, 20, 3, 'LeadWoken', ['READY'], 'Lead is active and monitoring execution.'),
      ev(5, 23, 23, 56, 'QuestionAsked', ['M2'], 'Soft-delete or hard-delete categories?', { attention_id: 'att-m2-q1' }),
    ];
    return d;
  }

  function seedStreamforge() {
    const d = baseStreamforge();
    d.workflow = workflowStreamforge();
    d.run = runStreamforge();
    d.events = [
      { seq: 1, ts: todayAt(21, 40, 2), type: 'MergeCompleted', milestone: 'M1', milestones: ['M1'], message: 'M1 merged into feature/backpressure.', acked: true },
      { seq: 2, ts: todayAt(22, 15, 0), type: 'VerificationFailed', milestone: 'M2', milestones: ['M2'], message: 'M2: 3 tests failing in metrics::tests.', acked: false, attention_id: 'att-sf-m2-fail' },
    ];
    return d;
  }

  /* ------------------------------------------------------------------ state */

  const store = {
    project: 'herdr-orchestra',
    projects: clone(PROJECTS),
    data: { 'herdr-orchestra': seedHerdrOrchestra(), streamforge: seedStreamforge() },
    models: clone(MODELS),
  };

  const listeners = new Set();
  const eventListeners = new Set();
  const pending = new Set();

  try {
    const saved = global.localStorage.getItem('herdr.project');
    if (saved && store.data[saved]) store.project = saved;
  } catch (_) { /* storage unavailable */ }

  const data = (id = store.project) => store.data[id];
  const notify = (reason) => { listeners.forEach((fn) => fn(reason)); scheduleSave(); };

  /* State survives page changes: every screen is its own document, so the store is saved to
     localStorage after each change and loaded again on startup. A version mismatch falls back to the seed. */
  const STATE_KEY = 'herdr.state.v1';
  const STATE_VERSION = 1;
  let saveTimer = null;

  function saveState() {
    try { global.localStorage.setItem(STATE_KEY, JSON.stringify({ v: STATE_VERSION, uid, data: store.data })); } catch (_) { /* storage unavailable */ }
  }
  function scheduleSave() {
    if (saveTimer) return;
    saveTimer = setTimeout(() => { saveTimer = null; saveState(); }, 50);
  }
  function hydrate() {
    try {
      const raw = global.localStorage.getItem(STATE_KEY);
      if (!raw) return;
      const saved = JSON.parse(raw);
      const ids = Object.keys(store.data);
      if (!saved || saved.v !== STATE_VERSION || !saved.data) return;
      if (!ids.every((id) => saved.data[id] && saved.data[id].status && saved.data[id].workflow && saved.data[id].run)) return;
      ids.forEach((id) => { saved.data[id].status.lead.typing = false; });
      store.data = saved.data;
      uid = Math.max(uid, Number(saved.uid) || 0);
    } catch (_) { /* corrupt or unavailable: keep the seed */ }
  }
  hydrate();

  function pushEvent(type, milestone, message, projectId = store.project) {
    const d = data(projectId);
    const ev = { seq: d.events.length + 1, ts: nowIso(), type, milestone: milestone || null, milestones: milestone ? [milestone] : [], message, acked: false };
    d.events.push(ev);
    eventListeners.forEach((fn) => fn(clone(ev), projectId));
    return ev;
  }

  function pushChat(role, text, extra = {}, projectId = store.project) {
    const d = data(projectId);
    const msg = {
      id: nextId('msg'), role, author: role === 'user' ? 'Arun N' : 'lead', ts: nowIso(), text, ...extra,
    };
    d.chat.push(msg);
    return msg;
  }

  function later(fn, ms) {
    const entry = { fn };
    entry.t = setTimeout(() => { pending.delete(entry); fn(); }, ms);
    pending.add(entry);
  }

  /* Run every scheduled reply right now, so leaving the page mid-reply does not lose it. */
  function flushPending() {
    [...pending].forEach((e) => { clearTimeout(e.t); pending.delete(e); e.fn(); });
  }
  global.addEventListener('pagehide', () => { flushPending(); saveState(); });

  function task(d, id) { return d.tasks.find((t) => t.milestone === id); }
  function agent(d, id) { return d.agents.find((a) => a.id === id); }
  function openAttention(d) { return d.attention.filter((a) => a.status === 'open'); }

  function summary(d) {
    const c = {};
    d.tasks.forEach((t) => { c[t.state] = (c[t.state] || 0) + 1; });
    return `${d.plan.milestones.length} milestones: ` + Object.entries(c).map(([k, v]) => `${v} ${k}`).join(', ');
  }

  function leadReply(text, projectId) {
    const d = data(projectId);
    d.status.lead.typing = true;
    notify('chat');
    later(() => {
      d.status.lead.typing = false;
      let reply;
      if (/status|progress|how.*going/i.test(text)) {
        reply = `Here is where we are — ${summary(d)}. ${openAttention(d).length} item(s) need your attention.`;
      } else if (/m4|hold|migration/i.test(text)) {
        const t = task(d, 'M4');
        reply = t && t.state === 'held'
          ? 'M4 is held because it edits a migration file. Approve it from the Attention Inbox and autopilot will pick it up once its dependencies merge.'
          : 'M4 is no longer held; it will start as soon as M1–M3 are merged.';
      } else if (/pause|stop|cancel/i.test(text)) {
        reply = 'Noted. I have relayed that through `orch chat`; autopilot will pause after the current milestones finish.';
      } else {
        reply = 'Understood. I have relayed that through `orch chat` and will adapt the plan if it changes scope.';
      }
      pushChat('lead', reply, {}, projectId);
      notify('chat');
    }, 900);
  }

  /* ------------------------------------------------------------------ REST-shaped reads */

  function snapshot(path) {
    const p = path.replace(/^https?:\/\/[^/]+/, '').replace(/\?.*$/, '').replace(/\/$/, '');
    const d = data();
    switch (p) {
      case `${API_BASE}/status`: return { ...d.status, counts: { attention: openAttention(d).length } };
      case `${API_BASE}/plan`: return { ...d.plan, workflow: d.workflow };
      case `${API_BASE}/tasks`: return d.tasks;
      case `${API_BASE}/events`: return d.events;
      case `${API_BASE}/attention`: return d.attention;
      case `${API_BASE}/chat`: return d.chat;
      case `${API_BASE}/agents`: return d.agents.map((a) => ({ ...a, output: a.output.map(stripAnsi) }));
      case `${API_BASE}/models`: return store.models;
      case `${API_BASE}/report`: {
        const by = {};
        d.tasks.forEach((t) => { by[t.state] = (by[t.state] || 0) + 1; });
        return { ...d.report, project: d.status.project, milestones: { total: d.tasks.length, by_state: by } };
      }
      default: return null;
    }
  }

  const api = {
    base: API_BASE,
    get(path) {
      const out = snapshot(path);
      return out === null
        ? Promise.reject(new Error(`404 ${path}`))
        : Promise.resolve(clone(out));
    },
  };

  /* ------------------------------------------------------------------ actions */

  const actions = {
    switchProject(id) {
      if (!store.data[id] || id === store.project) return;
      store.project = id;
      try { global.localStorage.setItem('herdr.project', id); } catch (_) { /* ignore */ }
      notify('project');
    },

    resolveAttention(itemId, optionId) {
      const d = data();
      const item = d.attention.find((a) => a.id === itemId);
      if (!item || item.status !== 'open') return null;
      const opt = item.options.find((o) => o.id === optionId);
      if (!opt) return null;
      item.status = 'resolved';
      item.resolution = { option_id: opt.id, label: opt.label, at: nowIso() };

      if (item.kind === 'hold' && opt.id === 'approve') {
        const t = task(d, item.milestone);
        if (t) t.state = 'queued';
        pushEvent('milestone.hold_released', item.milestone, `${item.milestone} hold approved; queued for autopilot`);
        pushChat('lead', `${item.milestone} hold approved. It is queued and will start once its dependencies have merged.`);
      } else if (item.kind === 'question') {
        const a = agent(d, item.agent);
        if (a) a.output.push(`\u001b[32m✓\u001b[0m Answer received: ${opt.label}`);
        pushEvent('question.answered', item.milestone, `${item.agent} answered: ${opt.label}`);
        pushChat('lead', `Relayed to ${item.agent}: "${opt.label}". ${item.milestone} continues with that decision.`);
      } else if (item.kind === 'failure' && opt.id === 'retry') {
        const t = task(d, item.milestone);
        if (t) { t.state = 'coding'; t.attempts += 1; }
        const a = agent(d, item.agent);
        if (a) { a.state = 'coding'; a.milestone = item.milestone; a.detail = `Working on ${item.milestone} · retry`; }
        pushEvent('milestone.retry', item.milestone, `${item.milestone} retry started`);
        pushChat('lead', `Retrying ${item.milestone} with the failing tests as context.`);
      } else {
        pushEvent('attention.dismissed', item.milestone, `${item.milestone} attention item dismissed`);
      }
      notify('attention');
      return item;
    },

    approvePlan() {
      const d = data();
      if (d.plan.approved) return;
      d.plan.approved = true;
      d.status.lead.state = 'running';
      pushEvent('plan.approved', null, `Plan "${d.plan.title}" approved — autopilot driving`);
      pushChat('lead', 'Plan approved. Autopilot is driving; M3 starts when M2 merges, and M4 stays held until you approve it.');
      notify('plan');
    },

    requestPlanChanges(text) {
      const pid = store.project;
      const d = data(pid);
      const body = String(text).trim();
      if (!body) return;
      pushChat('user', `Ask for changes: ${body}`);
      pushEvent('plan.changes_requested', null, 'Changes requested on the plan');
      d.status.lead.state = 'revising';
      notify('plan');
      later(() => {
        d.plan.revision += 1;
        d.status.lead.state = 'waiting_for_go';
        pushChat('lead', `Revised the plan (v${d.plan.revision}) to reflect that. Review the milestones and press Go when ready.`, {}, pid);
        notify('plan');
      }, 1400);
    },

    sendChat(text) {
      const body = String(text).trim();
      if (!body) return;
      pushChat('user', body);
      notify('chat');
      leadReply(body, store.project);
    },

    submitFeatureRequest(req) {
      const pid = store.project;
      const d = data(pid);
      const count = Math.min(4, Math.max(1, Number(req.milestones) || 4));
      const lines = [
        `Feature request: ${req.title}`,
        req.description,
        req.acceptance ? `Acceptance: ${req.acceptance}` : '',
        `Isolation: ${req.isolation} · Max parallel: ${req.max_parallel} · Tier: ${req.tier}${req.migrations ? ' · touches migrations' : ''}`,
      ].filter(Boolean);
      pushChat('user', lines.join('\n'), { kind: 'feature_request' });

      d.attention.forEach((a) => {
        if (a.status === 'open') {
          a.status = 'dismissed';
          a.resolution = { option_id: 'superseded', label: 'Superseded by new plan', at: nowIso() };
        }
      });
      d.agents.forEach((a) => {
        if (a.role !== 'lead') { a.state = 'idle'; a.milestone = null; a.task = null; a.detail = 'No active task · Awaiting assignment'; }
      });
      d.status.lead.state = 'planning';
      pushEvent('plan.requested', null, `Feature request: ${req.title}`);
      notify('chat');

      later(() => {
        const slug = String(req.title).toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 24) || 'feature';
        const titles = [
          `Design data model and migration for ${req.title}`,
          `Implement ${req.title} service and API`,
          `Build UI for ${req.title}`,
          `Add tests, seed data and docs for ${req.title}`,
        ];
        const picked = count === 4 ? titles : [...titles.slice(0, count - 1), titles[3]];
        const tierFor = (i) => (req.tier === 'auto' ? (i % 2 ? 'high' : 'medium') : req.tier);
        const id = nextId('plan');
        d.plan = {
          id, title: req.title, isolation: req.isolation, max_parallel: Number(req.max_parallel) || 2,
          approved: false, revision: 1,
          milestones: picked.map((title, i) => ({
            id: `M${i + 1}`, title, tier: tierFor(i),
            depends_on: i === 0 ? [] : [`M${i}`], branch: `orch/m${i + 1}-${slug}`,
          })),
        };
        d.tasks = d.plan.milestones.map((m) => ({ milestone: m.id, state: 'pending', agent: null, branch: m.branch, attempts: 0 }));

        /* the Workflows, Monitor and Overview screens read workflow + run, so rebuild those from the same plan */
        const prev = d.workflow;
        d.workflow = {
          title: req.title, base: prev.base, branch: prev.branch, check: prev.check, isolation: req.isolation,
          scope: prev.scope, risk_hold: prev.risk_hold || Boolean(req.migrations), commits: prev.commits, synced_at: nowIso(),
          milestones: d.plan.milestones.map((m, i) => ms(
            m.id, m.title, m.tier, 'pending', null, m.depends_on,
            i === 0 ? 'Foundation the other milestones build on.' : `Builds on ${m.depends_on.join(' + ')} and can be reviewed on its own.`,
            m.title,
            [`src/${slug}/mod.rs`],
            [[`${m.title} works as described in the request.`, false], [`Checks pass: ${prev.check}`, false]],
            ['// interfaces are defined by the lead during the first milestone'],
          )),
        };
        const run = d.run;
        run.id += 1;
        run.state = 'paused';
        run.plan = req.title;
        run.started_at = nowIso();
        run.percent = 0; run.merged = 0; run.coding = 0; run.total = picked.length;
        run.elapsed_s = 0; run.ticks = 0; run.rot = 0; run.script = {};
        run.agents.forEach((a) => {
          a.state = 'idle'; a.badge = 'IDLE';
          a.rows = [{ label: 'Task', kind: 'text', value: 'No active task' }];
        });
        run.outputs.autopilot.push({ ts: fmtClock(run.clock_s), text: `[autopilot] new plan "${req.title}" loaded; run #${run.id} is ready to start` });

        if (req.migrations) {
          const last = d.plan.milestones[0];
          task(d, last.id).state = 'held';
          d.attention.push({
            id: nextId('att'), kind: 'hold', milestone: last.id, agent: null,
            title: `${last.id} Held — Risk Hold: touches migration file`,
            body: `The milestone ${last.id} is held for approval because it modifies a migration file (schema change).`,
            options: [{ id: 'approve', label: `Approve ${last.id}`, icon: 'check', tone: 'amber' }],
            status: 'open', created_at: nowIso(), resolution: null,
          });
        }
        d.status.lead.state = 'waiting_for_go';
        pushEvent('plan.created', null, `Plan "${req.title}" created with ${picked.length} milestone(s)`, pid);
        pushChat('lead',
          `Got it. I've created an execution plan for "${req.title}" with ${picked.length} milestone${picked.length === 1 ? '' : 's'}. `
          + `The plan uses ${req.isolation} isolation and will run with a max parallel of ${d.plan.max_parallel} agents.`,
          { attachment: { type: 'plan', plan_id: id } }, pid);
        notify('plan');
      }, 1300);
    },

    openInHerdr(agentId) {
      const a = agent(data(), agentId);
      return a ? { pane: a.pane, agent: a.id } : null;
    },

    /* ---- workflow (screen 02) ---- */
    toggleMilestone(id) {
      const m = data().workflow.milestones.find((x) => x.id === id);
      if (!m) return;
      m.included = !m.included;
      notify('workflow');
    },
    toggleCriterion(msId, acId) {
      const m = data().workflow.milestones.find((x) => x.id === msId);
      const ac = m && m.criteria.find((c) => c.id === acId);
      if (!ac) return;
      ac.done = !ac.done;
      notify('workflow');
    },
    updateWorkflow(patch) {
      const wf = data().workflow;
      ['base', 'branch', 'check', 'isolation', 'scope', 'risk_hold', 'commits'].forEach((k) => {
        if (patch[k] !== undefined) wf[k] = patch[k];
      });
      data().status.branch = wf.branch;
      data().plan.isolation = wf.isolation;
      pushEvent('plan.updated', null, 'Plan settings edited');
      notify('workflow');
    },
    syncPlan() {
      data().workflow.synced_at = nowIso();
      pushEvent('plan.synced', null, 'Plan synced from plan file');
      notify('workflow');
    },

    /* ---- run (screen 03) ---- */
    setRunState(state) {
      const r = data().run;
      if (!['live', 'paused', 'stopped'].includes(state) || r.state === state) return false;
      r.state = state;
      const word = { live: 'resumed', paused: 'paused', stopped: 'stopped' }[state];
      r.outputs.autopilot.push({ ts: fmtClock(r.clock_s), text: `[autopilot] run #${r.id} ${word}` });
      pushEvent(`Run${word.charAt(0).toUpperCase()}${word.slice(1)}`, null, `Run #${r.id} ${word}`);
      notify('run');
      return true;
    },
    startRun() {
      const r = data().run;
      if (r.state === 'live') return false;
      r.state = 'live';
      r.outputs.autopilot.push({ ts: fmtClock(r.clock_s), text: `[autopilot] run #${r.id} started` });
      pushEvent('RunStarted', null, `Run #${r.id} started`);
      notify('run');
      return true;
    },
    /* Called once a second by the monitor; returns true when new log lines were appended. */
    tickRun() {
      const r = data().run;
      if (r.state !== 'live') return false;
      r.elapsed_s += 1;
      r.clock_s += 1;
      r.ticks += 1;
      if (r.ticks % 8) return false;
      const keys = Object.keys(r.script).filter((k) => r.script[k].length);
      if (!keys.length) return false;
      const key = keys[r.rot % keys.length];
      r.rot += 1;
      r.outputs[key].push({ ts: fmtClock(r.clock_s), text: r.script[key].shift() });
      notify('run');
      return true;
    },
    ackEvent(seq) {
      const e = data().events.find((x) => x.seq === seq);
      if (!e || e.acked) return;
      e.acked = true;
      notify('events');
    },
    replayEvent(seq) {
      const d = data();
      const e = d.events.find((x) => x.seq === seq);
      if (!e) return null;
      d.run.outputs.autopilot.push({ ts: fmtClock(d.run.clock_s), text: `[autopilot] replayed ${e.type} to lead` });
      notify('events');
      return e;
    },

    resetDemo() {
      pending.forEach((e) => clearTimeout(e.t));
      pending.clear();
      store.data = { 'herdr-orchestra': seedHerdrOrchestra(), streamforge: seedStreamforge() };
      notify('reset');
    },

    setTheme(theme) {
      const t = theme === 'light' ? 'light' : 'dark';
      document.documentElement.dataset.theme = t;
      try { global.localStorage.setItem('herdr.theme', t); } catch (_) { /* ignore */ }
      notify('theme');
      return t;
    },
  };

  /* ------------------------------------------------------------------ public API */

  global.HerdrStore = {
    api,
    actions,
    util: { stripAnsi, fmtTime },
    data,
    workflow: (id = store.project) => data(id).workflow,
    run: (id = store.project) => data(id).run,
    clockFmt: fmtClock,
    get project() { return store.project; },
    projectInfo: (id = store.project) => store.projects.find((p) => p.id === id),
    projects: () => store.projects.map((p) => ({
      ...p, attention: openAttention(store.data[p.id]).length, branch: store.data[p.id].status.branch,
    })),
    openAttention: (id = store.project) => openAttention(data(id)),
    theme: () => document.documentElement.dataset.theme || 'dark',
    /* Re-render hook: fn(reason) after any state change. Returns unsubscribe. */
    subscribe(fn) { listeners.add(fn); return () => listeners.delete(fn); },
    /* SSE-shaped hook for /api/v1/events: fn(event, projectId). */
    onEvent(fn) { eventListeners.add(fn); return () => eventListeners.delete(fn); },
  };
  global.OrchDemoStore = global.HerdrStore;
}(window));
