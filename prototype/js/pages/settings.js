/* Screen 08 — Settings. Sectioned runtime, agent, model and execution-policy configuration with dirty tracking and a TOML view. */
(function () {
  'use strict';

  const Store = window.OrchDemoStore;
  const UI = window.HerdrUI;
  const { h, iconEl, toast } = UI;
  const $ = (sel, root = document) => root.querySelector(sel);
  const clone = (v) => JSON.parse(JSON.stringify(v));

  /* ------------------------------------------------------------------ persistence */

  const key = (k) => `herdr.settings.${Store.project}.${k}`;
  const lsGet = (k, f) => { try { const v = localStorage.getItem(key(k)); return v === null ? f : JSON.parse(v); } catch (_) { return f; } };
  const lsSet = (k, v) => { try { localStorage.setItem(key(k), JSON.stringify(v)); } catch (_) { /* storage unavailable */ } };

  /* ------------------------------------------------------------------ defaults */

  const DEFAULTS = {
    ui_density: 'comfortable', timestamps: 'local', startup_page: '01-chat-home.html', reduce_motion: false,
    herdr_socket: '/run/user/1000/herdr.sock', herdr_bin: 'herdr', pane_layout: 'tiled', poll_ms: 1000, auto_reconnect: true,
    provider_claude: true, provider_codex: true, provider_amp: false, provider_opencode: false, provider_hermes: false, provider_antigravity: false,
    lead_model: 'claude:opus', coder_model: 'claude:sonnet', reviewer_model: 'claude:opus', reviewer_enabled: true, coder_effort: 'medium',
    tier_small: 'claude:haiku', tier_medium: 'claude:sonnet', tier_high: 'claude:opus', escalate_on_fail: true, fallback_model: 'codex:gpt-6-sol',
    lead_mode: 'supervised', confirm_finish: true, auto_approve_holds: false, chat_history_lines: 200,
    window_enabled: false, window_start: '22:00', window_end: '07:00', budget_hours: 3, idle_timeout_min: 20,
    isolation: 'worktree', max_parallel: 3, scope: 'warn', risk_hold: true, risk_delete: true, risk_migrations: true, risk_ci: true, risk_secrets: true,
    risk_max_lines: 1500, lead_wait_min: 15,
    base_branch: 'main', branch_prefix: 'orch/', commit_style: 'per-milestone', worktree_root: '../repo-wt', cleanup_worktrees: true, push_on_finish: false,
    check_cmd: 'cargo test && npm test', check_timeout_s: 600, verify_on_merge: true, fail_fast: true,
    require_review: true, review_strictness: 'blocking', suggestions_policy: 'report', review_diff_limit: 2000,
    risk_paths: 'migrations/**\n.github/workflows/**\n**/.env*',
    retry_fix: 3, retry_review: 2, retry_conflict: 2, retry_escalation: 1, auto_resume: true,
    notify_desktop: true, notify_hold: true, notify_question: true, notify_failure: true, notify_finish: true, notify_sound: false,
    state_dir: '.orchestrate', report_dir: '.orchestrate/reports', log_retention_days: 14, keep_worktrees_days: 3,
  };

  /* The project's current plan settings seed the first load, before anything has been saved. */
  function seeded() {
    const out = clone(DEFAULTS);
    const d = Store.data();
    const wf = d.workflow;
    out.isolation = wf.isolation;
    out.scope = wf.scope;
    out.risk_hold = wf.risk_hold;
    out.base_branch = wf.base;
    out.check_cmd = wf.check;
    out.commit_style = wf.commits;
    out.max_parallel = d.plan.max_parallel;
    out.herdr_socket = d.status.herdr.socket;
    return out;
  }

  /* ------------------------------------------------------------------ field catalogue */

  const MODEL_IDS = ['claude:opus', 'claude:sonnet', 'claude:haiku', 'codex:gpt-6-sol', 'codex:deepseek-v4-pro'];
  const modelOptions = () => MODEL_IDS.map((v) => ({ v, label: v }));

  const F = {
    theme: { type: 'seg', label: 'Theme', desc: 'Color theme for the whole app. Applies immediately and stays in sync with the top bar toggle.', icon: 'moon', env: 'applies immediately',
      options: [{ v: 'dark', label: 'Dark', icon: 'moon' }, { v: 'light', label: 'Light', icon: 'sun' }],
      live: { get: () => Store.theme(), set: (v) => Store.actions.setTheme(v) } },
    ui_density: { type: 'seg', label: 'Interface Density', desc: 'Spacing used in tables and lists.', icon: 'list', options: [{ v: 'comfortable', label: 'Comfortable' }, { v: 'compact', label: 'Compact' }] },
    timestamps: { type: 'seg', label: 'Timestamps', desc: 'Time zone used for events, logs and reports.', icon: 'clock', options: [{ v: 'local', label: 'Local time' }, { v: 'utc', label: 'UTC' }] },
    startup_page: { type: 'select', label: 'Startup Page', desc: 'Screen opened when the app starts.', icon: 'grid', options: [
      { v: '01-chat-home.html', label: 'Chat / Home' }, { v: '04-overview.html', label: 'Overview' }, { v: '03-execution-monitor.html', label: 'Execution Monitor' }, { v: '07-reports.html', label: 'Reports' }] },
    reduce_motion: { type: 'switch', label: 'Reduce Motion', desc: 'Skip pulsing indicators and transitions in this app.', icon: 'pulse' },

    herdr_socket: { type: 'text', label: 'Herdr Socket', desc: 'Unix socket the orchestrator uses to control panes.', icon: 'terminal', mono: true, env: 'HERDR_SOCKET_PATH' },
    herdr_bin: { type: 'text', label: 'Herdr Binary', desc: 'Command used to launch Herdr when it is not running.', icon: 'terminal', mono: true },
    pane_layout: { type: 'select', label: 'Pane Layout', desc: 'How new agent panes are arranged in a workspace.', icon: 'grid', options: [{ v: 'tiled', label: 'Tiled' }, { v: 'stacked', label: 'Stacked' }, { v: 'tabs', label: 'One tab per agent' }] },
    poll_ms: { type: 'step', label: 'Status Poll Interval', desc: 'How often pane state is polled when no event stream is available.', icon: 'sync', min: 250, max: 10000, step: 250, unit: 'ms' },
    auto_reconnect: { type: 'switch', label: 'Auto-Reconnect', desc: 'Reconnect to Herdr automatically after the socket drops.', icon: 'sync' },

    provider_claude: { type: 'switch', label: 'Claude', desc: 'Models: opus, sonnet, haiku. CLI detected on PATH.', icon: 'robot' },
    provider_codex: { type: 'switch', label: 'Codex', desc: 'Models: gpt-6-sol, deepseek-v4-pro. CLI detected on PATH.', icon: 'robot' },
    provider_amp: { type: 'switch', label: 'Amp', desc: 'CLI not detected on PATH. Enable after installing it.', icon: 'robot' },
    provider_opencode: { type: 'switch', label: 'OpenCode', desc: 'CLI not detected on PATH. Enable after installing it.', icon: 'robot' },
    provider_hermes: { type: 'switch', label: 'Hermes', desc: 'CLI not detected on PATH. Enable after installing it.', icon: 'robot' },
    provider_antigravity: { type: 'switch', label: 'Antigravity', desc: 'CLI not detected on PATH. Enable after installing it.', icon: 'robot' },

    lead_model: { type: 'select', label: 'Lead Agent', desc: 'Plans, answers questions and relays decisions.', icon: 'users', mono: true, options: modelOptions },
    coder_model: { type: 'select', label: 'Default Coder', desc: 'Used when a milestone does not request a tier.', icon: 'code', mono: true, options: modelOptions },
    reviewer_model: { type: 'select', label: 'Reviewer', desc: 'Reviews each milestone diff before merge.', icon: 'shield', mono: true, options: modelOptions },
    reviewer_enabled: { type: 'switch', label: 'Enable Reviewer', desc: 'When off, milestones merge after verification alone.', icon: 'shield' },
    coder_effort: { type: 'seg', label: 'Coder Effort', desc: 'Reasoning effort requested from coder agents.', icon: 'bolt', options: [{ v: 'low', label: 'Low' }, { v: 'medium', label: 'Medium' }, { v: 'high', label: 'High' }] },

    tier_small: { type: 'select', label: 'Tier: small', desc: 'Mechanical edits, docs, renames.', icon: 'package', mono: true, options: modelOptions },
    tier_medium: { type: 'select', label: 'Tier: medium', desc: 'Feature work with clear scope.', icon: 'package', mono: true, options: modelOptions },
    tier_high: { type: 'select', label: 'Tier: high', desc: 'Schema, API design, risky changes.', icon: 'package', mono: true, options: modelOptions },
    escalate_on_fail: { type: 'switch', label: 'Escalate on Failure', desc: 'Retry a failed milestone one tier higher before giving up.', icon: 'bolt' },
    fallback_model: { type: 'select', label: 'Fallback Model', desc: 'Used when the planned model is unavailable.', icon: 'robot', mono: true, options: modelOptions },

    lead_mode: { type: 'seg', label: 'Lead Mode', desc: 'Supervised asks you at decision points. Autonomous decides within policy.', icon: 'target', options: [{ v: 'supervised', label: 'Supervised' }, { v: 'autonomous', label: 'Autonomous' }] },
    confirm_finish: { type: 'switch', label: 'Ask Before Finish', desc: 'Require your approval before merging the feature branch and opening a PR.', icon: 'check' },
    auto_approve_holds: { type: 'switch', label: 'Auto-Approve Holds', desc: 'Release risk holds without asking. Not recommended for migrations.', icon: 'warning' },
    chat_history_lines: { type: 'step', label: 'Chat History Context', desc: 'Recent chat lines sent to the lead on each turn.', icon: 'chat', min: 50, max: 1000, step: 50, unit: 'lines' },

    window_enabled: { type: 'switch', label: 'Run Window', desc: 'Only dispatch new milestones inside a time window.', icon: 'clock' },
    window_start: { type: 'time', label: 'Window Start', desc: 'Local time when dispatching may begin.', icon: 'clock', mono: true },
    window_end: { type: 'time', label: 'Window End', desc: 'Local time when dispatching pauses.', icon: 'clock', mono: true },
    budget_hours: { type: 'step', label: 'Run Time Budget', desc: 'A run is paused when its active time passes this budget.', icon: 'clock', min: 1, max: 24, step: 1, unit: 'hours' },
    idle_timeout_min: { type: 'step', label: 'Idle Timeout', desc: 'Stop an agent that has produced no output for this long.', icon: 'clock', min: 5, max: 180, step: 5, unit: 'minutes' },

    isolation: { type: 'seg', label: 'Isolation Mode', desc: 'How agent workspaces are created and isolated.', icon: 'folder', options: [{ v: 'worktree', label: 'Worktree (Detached)', icon: 'folder' }, { v: 'shared', label: 'Shared Folder', icon: 'folder' }] },
    max_parallel: { type: 'step', label: 'Max Concurrent Coders', desc: 'Maximum number of coder agents that can run in parallel.', icon: 'users', min: 1, max: 8, step: 1 },
    scope: { type: 'select', label: 'Scope Enforcement', desc: 'Action to take when agents modify files outside the planned scope.', icon: 'target', mono: true, options: [
      { v: 'off', label: 'off (Ignore stray files)' }, { v: 'warn', label: 'warn (Report stray files)' }, { v: 'block', label: 'block (Reject the milestone)' }] },
    lead_wait_min: { type: 'step', label: 'Auto-Dispatch Lead Wait', desc: 'How long to wait for lead response before auto-dispatching the next action.', icon: 'clock', min: 1, max: 120, step: 1, unit: 'minutes' },

    risk_hold: { type: 'switch', label: 'Risk Hold Gate', desc: 'Hold milestones that touch risky or high-impact changes for explicit approval.' },
    risk_delete: { type: 'switch', label: 'Delete files', desc: '' },
    risk_migrations: { type: 'switch', label: 'Database migrations', desc: '' },
    risk_ci: { type: 'switch', label: 'CI configs', desc: '' },
    risk_secrets: { type: 'switch', label: 'Secrets files', desc: '' },
    risk_max_lines: { type: 'step', label: 'Line change limit', desc: '', min: 100, max: 20000, step: 100, unit: 'lines' },

    base_branch: { type: 'text', label: 'Base Branch', desc: 'Branch feature branches are created from and merged into.', icon: 'branch', mono: true },
    branch_prefix: { type: 'text', label: 'Milestone Branch Prefix', desc: 'Prefix for per-milestone branches.', icon: 'branch', mono: true },
    commit_style: { type: 'seg', label: 'Commit Style', desc: 'How milestone work lands on the feature branch.', icon: 'branch', options: [{ v: 'per-milestone', label: 'Per milestone' }, { v: 'squash', label: 'Squash all' }] },
    worktree_root: { type: 'text', label: 'Worktree Root', desc: 'Directory where agent worktrees are created.', icon: 'folder', mono: true },
    cleanup_worktrees: { type: 'switch', label: 'Clean Up Worktrees', desc: 'Remove milestone worktrees after they merge.', icon: 'trash' },
    push_on_finish: { type: 'switch', label: 'Push on Finish', desc: 'Push the feature branch to origin when a run finishes.', icon: 'external' },

    check_cmd: { type: 'text', label: 'Check Command', desc: 'Command that must pass before a milestone is reviewed.', icon: 'terminal', mono: true },
    check_timeout_s: { type: 'step', label: 'Check Timeout', desc: 'Fail the check when it runs longer than this.', icon: 'clock', min: 30, max: 3600, step: 30, unit: 'seconds' },
    verify_on_merge: { type: 'switch', label: 'Re-verify After Merge', desc: 'Run the check command again on the feature branch after each merge.', icon: 'checkcircle' },
    fail_fast: { type: 'switch', label: 'Fail Fast', desc: 'Stop the check at its first failing command.', icon: 'warning' },

    require_review: { type: 'switch', label: 'Require Review', desc: 'Every milestone needs reviewer approval before merging.', icon: 'chat' },
    review_strictness: { type: 'seg', label: 'Review Strictness', desc: 'Blocking issues stop a merge. Suggestions never do.', icon: 'shield', options: [{ v: 'blocking', label: 'Blocking only' }, { v: 'strict', label: 'Blocking + warnings' }] },
    suggestions_policy: { type: 'seg', label: 'Reviewer Suggestions', desc: 'What happens to non-blocking suggestions.', icon: 'chat', options: [{ v: 'report', label: 'Report only' }, { v: 'follow-up', label: 'Open follow-up fix' }] },
    review_diff_limit: { type: 'step', label: 'Review Diff Limit', desc: 'Largest diff, in changed lines, sent to the reviewer in one pass.', icon: 'doc', min: 200, max: 10000, step: 200, unit: 'lines' },

    risk_paths: { type: 'area', label: 'Protected Paths', desc: 'One glob per line. Changes to these paths are held for approval.', icon: 'shieldalert', mono: true, list: true },

    retry_fix: { type: 'step', label: 'Fix Rounds', desc: 'Attempts to repair a failed check before escalating.', icon: 'pencil', min: 1, max: 10, step: 1, unit: 'rounds' },
    retry_review: { type: 'step', label: 'Review Rounds', desc: 'Review cycles before the milestone is flagged for you.', icon: 'chat', min: 1, max: 10, step: 1, unit: 'rounds' },
    retry_conflict: { type: 'step', label: 'Conflict Rounds', desc: 'Attempts to resolve a merge conflict automatically.', icon: 'branch', min: 1, max: 10, step: 1, unit: 'rounds' },
    retry_escalation: { type: 'step', label: 'Escalations', desc: 'Times a milestone may move to a higher model tier.', icon: 'warning', min: 0, max: 3, step: 1 },
    auto_resume: { type: 'switch', label: 'Auto-Resume After Crash', desc: 'Resume an interrupted run from the last merged milestone.', icon: 'play' },

    notify_desktop: { type: 'switch', label: 'Desktop Notifications', desc: 'Show system notifications when the app is in the background.', icon: 'bell' },
    notify_hold: { type: 'switch', label: 'Risk Holds', desc: 'Notify when a milestone is held for approval.', icon: 'warning' },
    notify_question: { type: 'switch', label: 'Agent Questions', desc: 'Notify when an agent is waiting on a decision.', icon: 'question' },
    notify_failure: { type: 'switch', label: 'Failures', desc: 'Notify when a milestone fails after its retries.', icon: 'warning' },
    notify_finish: { type: 'switch', label: 'Run Finished', desc: 'Notify when a run reaches ALL_DONE.', icon: 'checkcircle' },
    notify_sound: { type: 'switch', label: 'Play Sound', desc: 'Play a short sound with notifications.', icon: 'bell' },

    state_dir: { type: 'text', label: 'State Directory', desc: 'Where plans, events and agent state are stored.', icon: 'folder', mono: true },
    report_dir: { type: 'text', label: 'Report Directory', desc: 'Where run reports and summary.md files are written.', icon: 'doc', mono: true },
    log_retention_days: { type: 'step', label: 'Log Retention', desc: 'Captured agent output older than this is deleted.', icon: 'clock', min: 1, max: 365, step: 1, unit: 'days' },
    keep_worktrees_days: { type: 'step', label: 'Keep Failed Worktrees', desc: 'Failed milestone worktrees are kept this long for inspection.', icon: 'clock', min: 0, max: 30, step: 1, unit: 'days' },
  };
  Object.entries(F).forEach(([k, f]) => { f.key = k; if (!f.env) f.env = `ORCH_${k.toUpperCase()}`; });

  const item = (k, extra) => ({ ...F[k], ...extra });
  const RISKS = [['risk_delete', 'Delete files'], ['risk_migrations', 'Database migrations'], ['risk_ci', 'CI configs'], ['risk_secrets', 'Secrets files']];

  const SECTIONS = [
    { id: 'general', label: 'General', icon: 'gear', title: 'General', desc: 'Appearance and everyday behavior of the desktop app.', table: 'general',
      items: ['theme', 'ui_density', 'timestamps', 'startup_page', 'reduce_motion'].map((k) => item(k)) },
    { id: 'runtime', label: 'Herdr Runtime', icon: 'terminal', title: 'Herdr Runtime', desc: 'How the orchestrator connects to and drives Herdr panes.', table: 'runtime',
      items: ['herdr_socket', 'herdr_bin', 'pane_layout', 'poll_ms', 'auto_reconnect'].map((k) => item(k)) },
    { id: 'providers', label: 'Agent Providers', icon: 'users', title: 'Agent Providers', desc: 'Which agent CLIs may be assigned to coder, reviewer and lead roles.', table: 'providers',
      items: ['provider_claude', 'provider_codex', 'provider_amp', 'provider_opencode', 'provider_hermes', 'provider_antigravity'].map((k) => item(k)) },
    { id: 'roles', label: 'Agent Roles', icon: 'users', title: 'Agent Roles', desc: 'Which model fills each role by default.', table: 'roles',
      items: ['lead_model', 'coder_model', 'reviewer_model', 'reviewer_enabled', 'coder_effort'].map((k) => item(k)) },
    { id: 'models', label: 'Models & Policy', icon: 'package', title: 'Models & Policy', desc: 'Map planned model tiers to concrete models and set the escalation policy.', table: 'models',
      items: ['tier_small', 'tier_medium', 'tier_high', 'escalate_on_fail', 'fallback_model'].map((k) => item(k)) },
    { id: 'lead', label: 'Lead Mode', icon: 'target', title: 'Lead Mode', desc: 'How much the lead agent may decide without asking you.', table: 'lead',
      items: ['lead_mode', 'confirm_finish', 'auto_approve_holds', 'chat_history_lines'].map((k) => item(k)) },
    { id: 'scheduling', label: 'Scheduling', icon: 'clock', title: 'Scheduling', desc: 'When runs may dispatch work and how long they may take.', table: 'schedule',
      items: ['window_enabled', 'window_start', 'window_end', 'budget_hours', 'idle_timeout_min'].map((k) => item(k)) },
    { id: 'parallel', label: 'Parallel Execution', icon: 'layers', title: 'Parallel Execution, Isolation & Risk Hold',
      desc: 'Control how agents run in parallel, workspace isolation, scope enforcement and when to hold risky changes.', table: 'parallel',
      items: [item('isolation'), item('max_parallel'), item('scope'), { custom: 'riskhold', keys: ['risk_hold', 'risk_delete', 'risk_migrations', 'risk_ci', 'risk_secrets', 'risk_max_lines'] }, item('lead_wait_min'), { custom: 'retries', keys: [] }] },
    { id: 'git', label: 'Git & Worktrees', icon: 'branch', title: 'Git & Worktrees', desc: 'Branching, commit and worktree conventions.', table: 'git',
      items: ['base_branch', 'branch_prefix', 'commit_style', 'worktree_root', 'cleanup_worktrees', 'push_on_finish'].map((k) => item(k)) },
    { id: 'verification', label: 'Verification', icon: 'shield', title: 'Verification', desc: 'The check that must pass before a milestone moves to review.', table: 'verification',
      items: ['check_cmd', 'check_timeout_s', 'verify_on_merge', 'fail_fast'].map((k) => item(k)) },
    { id: 'review', label: 'Review', icon: 'chat', title: 'Review', desc: 'Reviewer behavior and what happens to its suggestions.', table: 'review',
      items: ['require_review', 'review_strictness', 'suggestions_policy', 'review_diff_limit'].map((k) => item(k)) },
    { id: 'risk', label: 'Risk Hold & Scope', icon: 'warning', title: 'Risk Hold & Scope', desc: 'Which changes are held for approval or flagged as out of scope. Shared with Parallel Execution.', table: 'risk',
      items: [item('scope', { dup: true }), { custom: 'riskhold', dup: true, keys: ['risk_hold', 'risk_delete', 'risk_migrations', 'risk_ci', 'risk_secrets', 'risk_max_lines'] }, item('risk_paths')] },
    { id: 'recovery', label: 'Recovery & Retries', icon: 'sync', title: 'Recovery & Retries', desc: 'Bounded retry limits and crash recovery.', table: 'recovery',
      items: ['retry_fix', 'retry_review', 'retry_conflict', 'retry_escalation', 'auto_resume'].map((k) => item(k)) },
    { id: 'notifications', label: 'Notifications', icon: 'bell', title: 'Notifications', desc: 'Which events reach you outside the app.', table: 'notifications',
      items: ['notify_desktop', 'notify_hold', 'notify_question', 'notify_failure', 'notify_finish', 'notify_sound'].map((k) => item(k)) },
    { id: 'storage', label: 'Storage', icon: 'package', title: 'Storage', desc: 'Where orchestrator data lives and how long it is kept.', table: 'storage',
      items: [...['state_dir', 'report_dir', 'log_retention_days', 'keep_worktrees_days'].map((k) => item(k)), { custom: 'cache', keys: [] }] },
    { id: 'toml', label: 'Advanced TOML', icon: 'code', title: 'Advanced TOML', desc: 'The full configuration as it will be written to config.toml. Edit it directly and apply.', table: null,
      items: [{ custom: 'toml', keys: [] }] },
  ];
  SECTIONS.forEach((s) => {
    s.keys = [...new Set(s.items.flatMap((i) => (i.custom ? i.keys : [i.key])))];
    s.tomlKeys = s.items.filter((i) => !i.dup).flatMap((i) => (i.custom ? i.keys : [i.key])).filter((k) => !F[k].live);
  });

  /* ------------------------------------------------------------------ state */

  let saved = {};
  let draft = {};
  let current = SECTIONS[0].id;
  let live = [];

  const diffKeys = () => Object.keys(DEFAULTS).filter((k) => JSON.stringify(saved[k]) !== JSON.stringify(draft[k]));
  const isDirty = () => diffKeys().length > 0;

  function load() {
    saved = { ...seeded(), ...lsGet('values', {}) };
    draft = clone(saved);
  }

  function setValue(k, v) {
    draft[k] = v;
    changed();
  }

  function changed() {
    live.forEach((f) => f.sync());
    refreshMeta();
  }

  /* ------------------------------------------------------------------ controls */

  let uid = 0;

  function switchEl(k, labelId, extra = {}) {
    const lab = h('span', { class: 'switch__state', 'aria-hidden': 'true' });
    const btn = h('button', {
      type: 'button', class: 'switch', role: 'switch', 'aria-labelledby': labelId, dataset: { fk: `f-${k}` },
      on: { click: () => setValue(k, !draft[k]) },
    }, h('span', { class: 'switch__knob', 'aria-hidden': 'true' }));
    return {
      el: h('span', { class: 'switch-wrap' }, btn, lab),
      sync() { const on = Boolean(draft[k]); btn.setAttribute('aria-checked', String(on)); lab.textContent = extra.labels ? extra.labels[on ? 0 : 1] : (on ? 'ON' : 'OFF'); },
    };
  }

  function stepEl(f, labelId) {
    const k = f.key;
    const input = h('input', {
      class: 'input mono stepper__in', type: 'number', inputmode: 'numeric', min: f.min, max: f.max, step: f.step, 'aria-labelledby': labelId, dataset: { fk: `f-${k}` },
    });
    const clamp = (n) => Math.min(f.max, Math.max(f.min, n));
    const bump = (dir) => setValue(k, clamp(Number(draft[k]) + dir * f.step));
    input.addEventListener('input', () => { const n = Number(input.value); if (input.value !== '' && Number.isFinite(n) && n >= f.min && n <= f.max) setValue(k, n); });
    input.addEventListener('change', () => { const n = Number(input.value); setValue(k, Number.isFinite(n) && input.value !== '' ? clamp(Math.round(n)) : draft[k]); input.value = draft[k]; });
    const minus = h('button', { type: 'button', class: 'stepper__btn', 'aria-label': `Decrease ${f.label}`, dataset: { fk: `f-${k}-dec` }, on: { click: () => bump(-1) } }, '−');
    const plus = h('button', { type: 'button', class: 'stepper__btn', 'aria-label': `Increase ${f.label}`, dataset: { fk: `f-${k}-inc` }, on: { click: () => bump(1) } }, '+');
    return {
      el: h('span', { class: 'stepper-wrap' }, h('span', { class: 'stepper' }, minus, input, plus), f.unit ? h('span', { class: 'unit' }, f.unit) : null),
      sync() {
        if (document.activeElement !== input) input.value = draft[k];
        minus.disabled = draft[k] <= f.min; plus.disabled = draft[k] >= f.max;
      },
    };
  }

  function buildControl(f, labelId) {
    const k = f.key;
    if (f.type === 'switch') return switchEl(k, labelId);
    if (f.type === 'step') return stepEl(f, labelId);
    if (f.type === 'seg') {
      const val = () => (f.live ? f.live.get() : draft[k]);
      const btns = f.options.map((o) => h('button', {
        type: 'button', dataset: { fk: `f-${k}-${o.v}` }, 'aria-pressed': 'false',
        on: { click: () => { if (f.live) { f.live.set(o.v); changed(); } else setValue(k, o.v); } },
      }, o.icon ? iconEl(o.icon) : null, o.label));
      return { el: h('div', { class: 'seg', role: 'group', 'aria-labelledby': labelId }, btns), sync() { btns.forEach((b, i) => b.setAttribute('aria-pressed', String(f.options[i].v === val()))); } };
    }
    if (f.type === 'select') {
      const opts = typeof f.options === 'function' ? f.options() : f.options;
      const select = h('select', { class: `select${f.mono ? ' mono' : ''}`, 'aria-labelledby': labelId, dataset: { fk: `f-${k}` } },
        opts.map((o) => h('option', { value: o.v }, o.label)));
      if (!opts.some((o) => o.v === draft[k])) select.append(h('option', { value: draft[k] }, draft[k]));
      select.addEventListener('change', () => setValue(k, select.value));
      return { el: select, sync() { select.value = draft[k]; } };
    }
    if (f.type === 'area') {
      const ta = h('textarea', { class: 'textarea mono', rows: 4, spellcheck: 'false', 'aria-labelledby': labelId, dataset: { fk: `f-${k}` } });
      ta.addEventListener('input', () => setValue(k, ta.value));
      return { el: ta, sync() { if (document.activeElement !== ta) ta.value = draft[k]; } };
    }
    const input = h('input', {
      class: `input${f.mono || f.type === 'time' ? ' mono' : ''}`, type: f.type === 'time' ? 'time' : 'text', spellcheck: 'false', autocomplete: 'off',
      'aria-labelledby': labelId, dataset: { fk: `f-${k}` },
    });
    input.addEventListener('input', () => { if (input.value.trim() !== '' || f.type !== 'time') setValue(k, input.value); });
    return { el: input, sync() { if (document.activeElement !== input) input.value = draft[k]; } };
  }

  /* ------------------------------------------------------------------ rows */

  function rowShell(f, keys, control, labelId, opts = {}) {
    const mod = h('span', { class: 'badge badge--amber modbadge', hidden: true }, 'modified');
    const row = h('div', { class: `srow${opts.wide ? ' srow--wide' : ''}`, dataset: { keys: keys.join(' ') } },
      h('span', { class: 'srow__icon', 'aria-hidden': 'true' }, iconEl(f.icon || 'gear')),
      h('div', { class: 'srow__label' }, h('div', { class: 'srow__name', id: labelId }, f.label, ' ', mod), h('p', { class: 'srow__desc' }, f.desc)),
      h('div', { class: 'srow__ctl' }, control),
      f.env ? h('code', { class: 'env mono' }, f.env) : h('span'));
    return { row, mod, keys };
  }

  function fieldRow(f) {
    const id = `lbl-${++uid}`;
    const c = buildControl(f, id);
    const { row, mod } = rowShell(f, [f.key], c.el, id);
    live.push({ sync() { c.sync(); mod.hidden = f.live ? true : JSON.stringify(saved[f.key]) === JSON.stringify(draft[f.key]); row.classList.toggle('is-dirty', !mod.hidden); } });
    return row;
  }

  function riskRow(it) {
    const id = `lbl-${++uid}`;
    const f = { label: 'Risk Hold Gate', desc: 'Hold milestones that touch risky or high-impact changes for explicit approval.', icon: 'shieldalert', env: 'ORCH_RISK_HOLD' };
    const sw = switchEl('risk_hold', id);
    const boxes = RISKS.map(([k, label]) => {
      const cb = h('input', { type: 'checkbox', dataset: { fk: `f-${k}` }, on: { change: () => setValue(k, cb.checked) } });
      return { k, cb, el: h('label', { class: 'check riskbox' }, cb, h('span', null, label)) };
    });
    const lid = `lbl-${++uid}`;
    const lines = stepEl({ ...F.risk_max_lines, unit: '' }, lid);
    const sub = h('fieldset', { class: 'risk__sub' },
      h('legend', { class: 'sr-only' }, 'Risk hold triggers'),
      h('div', { class: 'risk__checks' }, boxes.map((b) => b.el)),
      h('div', { class: 'risk__lines' }, h('span', { id: lid }, 'Line change limit (auto-hold if exceeds):'), lines.el, h('code', { class: 'env mono' }, 'ORCH_RISK_MAX_LINES')));
    const { row, mod } = rowShell(f, it.keys, h('div', { class: 'risk' }, sw.el, sub), id, { wide: true });
    live.push({
      sync() {
        sw.sync(); lines.sync();
        boxes.forEach((b) => { b.cb.checked = Boolean(draft[b.k]); });
        sub.disabled = !draft.risk_hold;
        const dirty = it.keys.some((k) => JSON.stringify(saved[k]) !== JSON.stringify(draft[k]));
        mod.hidden = !dirty; row.classList.toggle('is-dirty', dirty);
      },
    });
    return row;
  }

  const CHIPS = [
    ['retry_fix', 'Fix', 'rounds', 'cyan', 'pencil'], ['retry_review', 'Review', 'rounds', 'violet', 'chat'],
    ['retry_conflict', 'Conflict', 'rounds', 'amber', 'branch'], ['retry_escalation', 'Escalation', '', 'coral', 'warning'],
  ];

  function retriesRow() {
    const id = `lbl-${++uid}`;
    const f = { label: 'Bounded Retry Caps', desc: 'Shipped limits for retries and escalation (read-only here). Change them in Recovery & Retries.', icon: 'sync', env: null };
    const chips = CHIPS.map(([k, label, unit, tone, icon]) => ({ k, label, unit, el: h('span', { class: `badge badge--${tone} chip-cap` }, iconEl(icon, 14), h('span')) }));
    const { row } = rowShell(f, [], h('div', { class: 'caps', role: 'list', 'aria-labelledby': id }, chips.map((c) => { c.el.setAttribute('role', 'listitem'); return c.el; })), id, { wide: true });
    live.push({ sync() { chips.forEach((c) => { c.el.lastElementChild.textContent = `${c.label}: ${draft[c.k]}${c.unit ? ` ${c.unit}` : ''}`; }); } });
    return row;
  }

  function cacheRow() {
    const id = `lbl-${++uid}`;
    const btn = h('button', { type: 'button', class: 'btn btn--coral', 'aria-haspopup': 'dialog', dataset: { fk: 'clear-cache' }, on: { click: () => { $('#cache-path').textContent = `${draft.state_dir}/logs`; $('#cache-dialog').showModal(); } } }, iconEl('trash'), 'Clear cached logs');
    const { row } = rowShell({ label: 'Cached Agent Logs', desc: 'Captured plain-text pane output kept for the log viewer.', icon: 'trash', env: null }, [], btn, id);
    return row;
  }

  /* ------------------------------------------------------------------ TOML */

  const quote = (s) => `"${String(s).replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
  function tomlValue(f, v) {
    if (f.list) return `[${String(v).split('\n').map((s) => s.trim()).filter(Boolean).map(quote).join(', ')}]`;
    return f.type === 'switch' || f.type === 'step' ? String(v) : quote(v);
  }

  function toToml() {
    const out = ['# .orchestrate/config.toml', '# Precedence: CLI > Env > Plan > .orchestrate/config.toml > Defaults', ''];
    SECTIONS.filter((s) => s.table).forEach((s) => {
      const keys = s.tomlKeys;
      if (!keys.length) return;
      out.push(`[${s.table}]`);
      keys.forEach((k) => out.push(`${k} = ${tomlValue(F[k], draft[k])}`));
      out.push('');
    });
    return out.join('\n');
  }

  function parseToml(text) {
    const errors = [];
    const next = {};
    let table = null;
    const tableOf = (name) => SECTIONS.find((s) => s.table === name);
    text.split('\n').forEach((raw, i) => {
      const line = raw.trim();
      if (!line || line.startsWith('#')) return;
      const t = /^\[([a-z_]+)\]$/.exec(line);
      if (t) { table = tableOf(t[1]); if (!table) errors.push(`Line ${i + 1}: unknown table [${t[1]}]`); return; }
      const kv = /^([a-z_]+)\s*=\s*(.+)$/.exec(line);
      if (!kv) { errors.push(`Line ${i + 1}: expected key = value`); return; }
      if (!table) { errors.push(`Line ${i + 1}: key outside a table`); return; }
      const [, k, rawV] = kv;
      const f = F[k];
      if (!f || !table.tomlKeys.includes(k)) { errors.push(`Line ${i + 1}: unknown key ${table.table}.${k}`); return; }
      const v = rawV.trim();
      if (f.type === 'switch') {
        if (v !== 'true' && v !== 'false') errors.push(`Line ${i + 1}: ${k} must be true or false`); else next[k] = v === 'true';
      } else if (f.type === 'step') {
        const n = Number(v);
        if (!Number.isInteger(n) || n < f.min || n > f.max) errors.push(`Line ${i + 1}: ${k} must be a whole number from ${f.min} to ${f.max}`); else next[k] = n;
      } else if (f.list) {
        const items = [...v.matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((m) => m[1].replace(/\\(.)/g, '$1'));
        if (!/^\[.*\]$/.test(v)) errors.push(`Line ${i + 1}: ${k} must be an array of strings`); else next[k] = items.join('\n');
      } else {
        const s = /^"((?:[^"\\]|\\.)*)"$/.exec(v);
        if (!s) { errors.push(`Line ${i + 1}: ${k} must be a quoted string`); return; }
        const val = s[1].replace(/\\(.)/g, '$1');
        const opts = typeof f.options === 'function' ? f.options() : f.options;
        if (f.type === 'seg' || f.type === 'select') {
          if (opts.some((o) => o.v === val) || f.type === 'select' && typeof f.options === 'function') next[k] = val; else errors.push(`Line ${i + 1}: ${k} must be one of ${opts.map((o) => o.v).join(', ')}`);
        } else next[k] = val;
      }
    });
    return { errors, next };
  }

  function download(name, text, type) {
    const url = URL.createObjectURL(new Blob([text], { type }));
    const a = h('a', { href: url, download: name, hidden: true });
    document.body.append(a); a.click(); a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  function tomlRow() {
    const id = `lbl-${++uid}`;
    const ta = h('textarea', { class: 'textarea mono toml', rows: 20, spellcheck: 'false', 'aria-labelledby': id, dataset: { fk: 'toml-editor' } });
    const msg = h('div', { class: 'toml__msg', role: 'alert' });
    const fill = () => { ta.value = toToml(); msg.replaceChildren(); msg.className = 'toml__msg'; };
    fill();
    const apply = () => {
      const { errors, next } = parseToml(ta.value);
      if (errors.length) {
        msg.className = 'toml__msg toml__msg--bad';
        msg.replaceChildren(h('strong', null, `${errors.length} problem${errors.length === 1 ? '' : 's'} found. Nothing was applied.`), h('ul', null, errors.slice(0, 8).map((e) => h('li', { class: 'mono' }, e))));
        return;
      }
      Object.assign(draft, next);
      changed();
      const n = Object.keys(next).length;
      msg.className = 'toml__msg toml__msg--ok';
      msg.replaceChildren(`Applied ${n} value${n === 1 ? '' : 's'} to the form. Press Save to write config.toml.`);
      toast('TOML applied to the form', 'cyan', 'check');
    };
    const copy = async () => {
      try { await navigator.clipboard.writeText(ta.value); toast('Copied config.toml', 'cyan', 'copy'); } catch (_) { toast('Copy is blocked in this context. Select the text and copy it manually.', 'amber', 'warning'); }
    };
    const row = h('div', { class: 'srow srow--wide srow--toml' },
      h('div', { class: 'srow__label srow__label--full' }, h('div', { class: 'srow__name', id }, 'config.toml'), h('p', { class: 'srow__desc' }, 'Values stored in the project at .orchestrate/config.toml. Theme is a UI preference and is not part of the file.')),
      ta, msg,
      h('div', { class: 'toml__btns' },
        h('button', { type: 'button', class: 'btn btn--violet', dataset: { fk: 'toml-apply' }, on: { click: apply } }, iconEl('check'), 'Apply TOML to form'),
        h('button', { type: 'button', class: 'btn', dataset: { fk: 'toml-revert' }, on: { click: fill } }, iconEl('sync'), 'Revert to form values'),
        h('button', { type: 'button', class: 'btn', dataset: { fk: 'toml-copy' }, on: { click: copy } }, iconEl('copy'), 'Copy'),
        h('button', { type: 'button', class: 'btn', dataset: { fk: 'toml-dl' }, on: { click: () => { download('config.toml', ta.value, 'text/plain'); toast('Downloaded config.toml', 'emerald', 'check'); } } }, iconEl('external'), 'Download')));
    return row;
  }

  /* ------------------------------------------------------------------ section rendering */

  const navButtons = new Map();

  function sectionDirty(s) { return s.keys.some((k) => JSON.stringify(saved[k]) !== JSON.stringify(draft[k])); }

  function buildNav() {
    const nav = $('#set-nav');
    nav.replaceChildren(...SECTIONS.map((s) => {
      const dot = h('span', { class: 'dot dot--amber navdot', hidden: true }, '');
      const sr = h('span', { class: 'sr-only', hidden: true }, ' (unsaved changes)');
      const b = h('button', {
        type: 'button', class: 'navtab', role: 'tab', id: `tab-${s.id}`, 'aria-controls': 'panel-rows', 'aria-selected': 'false', tabindex: '-1', dataset: { section: s.id },
        on: { click: () => selectSection(s.id) },
      }, iconEl(s.icon), h('span', { class: 'navtab__label' }, s.label), dot, sr);
      navButtons.set(s.id, { b, dot, sr });
      return b;
    }));
    nav.addEventListener('keydown', (e) => {
      const ids = SECTIONS.map((s) => s.id);
      let i = ids.indexOf(current);
      if (e.key === 'ArrowDown' || e.key === 'ArrowRight') i = (i + 1) % ids.length;
      else if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') i = (i - 1 + ids.length) % ids.length;
      else if (e.key === 'Home') i = 0;
      else if (e.key === 'End') i = ids.length - 1;
      else return;
      e.preventDefault();
      selectSection(ids[i]);
      navButtons.get(ids[i]).b.focus();
    });
  }

  function selectSection(id) {
    const s = SECTIONS.find((x) => x.id === id) || SECTIONS[0];
    current = s.id;
    lsSet('section', current);
    try { history.replaceState(null, '', `#${current}`); } catch (_) { /* file:// restrictions */ }
    navButtons.forEach((n, sid) => { n.b.setAttribute('aria-selected', String(sid === current)); n.b.tabIndex = sid === current ? 0 : -1; });
    $('#panel-icon').replaceChildren(iconEl(s.icon));
    $('#panel-title').textContent = s.title;
    $('#panel-desc').textContent = s.desc;
    $('#panel-prec').textContent = 'Precedence: CLI > Env > Plan > .orchestrate/config.toml > Defaults';
    const rows = $('#panel-rows');
    rows.setAttribute('aria-labelledby', `tab-${s.id}`);
    live = [];
    rows.replaceChildren(...s.items.map((it) => {
      if (it.custom === 'riskhold') return riskRow(it);
      if (it.custom === 'retries') return retriesRow();
      if (it.custom === 'cache') return cacheRow();
      if (it.custom === 'toml') return tomlRow();
      return fieldRow(it);
    }));
    changed();
  }

  function refreshMeta() {
    const n = diffKeys().length;
    const dirty = n > 0;
    $('#dirty').className = `dirty${dirty ? ' dirty--on' : ''}`;
    $('#dirty').replaceChildren(dirty ? h('span', { class: 'dot dot--amber', 'aria-hidden': 'true' }) : iconEl('check', 14),
      dirty ? `${n} unsaved change${n === 1 ? '' : 's'}` : 'All changes saved');
    $('#save-btn').disabled = !dirty;
    $('#discard-btn').disabled = !dirty;
    SECTIONS.forEach((s) => { const d = sectionDirty(s); const n2 = navButtons.get(s.id); n2.dot.hidden = !d; n2.sr.hidden = !d; });
  }

  /* ------------------------------------------------------------------ save / reset / discard */

  function save() {
    if (!isDirty()) return;
    const planKeys = ['isolation', 'scope', 'risk_hold', 'base_branch', 'check_cmd', 'commit_style'];
    const planChanged = planKeys.some((k) => saved[k] !== draft[k]);
    saved = clone(draft);
    lsSet('values', saved);
    if (planChanged) {
      Store.actions.updateWorkflow({ isolation: saved.isolation, scope: saved.scope, risk_hold: saved.risk_hold, base: saved.base_branch, check: saved.check_cmd, commits: saved.commit_style });
    }
    changed();
    toast('Saved to .orchestrate/config.toml', 'emerald', 'check');
  }

  function discard() {
    draft = clone(saved);
    changed();
    toast('Unsaved changes discarded', 'cyan', 'sync');
  }

  function resetDefaults() {
    draft = clone(DEFAULTS);
    changed();
    toast('Defaults restored. Press Save to write them.', 'amber', 'sync');
  }

  /* ------------------------------------------------------------------ wiring */

  function wire() {
    $('#set-icon').replaceChildren(iconEl('gear'));
    $('#save-btn').replaceChildren(iconEl('doc'), 'Save to config.toml');
    $('#reset-btn').replaceChildren(iconEl('sync'), 'Reset to Defaults');
    $('#discard-btn').replaceChildren(iconEl('x'), 'Discard changes');
    $('#save-btn').addEventListener('click', save);
    $('#discard-btn').addEventListener('click', discard);
    $('#reset-btn').addEventListener('click', () => $('#reset-dialog').showModal());
    $('#reset-dialog').addEventListener('close', (e) => { if (e.target.returnValue === 'reset') resetDefaults(); });
    $('#cache-dialog').addEventListener('close', (e) => { if (e.target.returnValue === 'clear') toast(`Cleared cached logs in ${draft.state_dir}/logs`, 'coral', 'trash'); });
    ['#reset-dialog', '#cache-dialog'].forEach((s) => UI.enhanceDialog($(s)));
    window.addEventListener('beforeunload', (e) => { if (isDirty()) { e.preventDefault(); e.returnValue = ''; } });
    window.addEventListener('keydown', (e) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') { e.preventDefault(); save(); }
    });
  }

  const go = (id, label) => ({ group: 'Settings', label: `Go to ${label}`, icon: 'gear', hint: id, run: () => { selectSection(id); navButtons.get(id).b.focus(); } });
  window.HerdrShell.registerCommands([
    { group: 'Settings', label: 'Save settings to config.toml', icon: 'doc', hint: 'Ctrl+S', run: save },
    { group: 'Settings', label: 'Reset settings to defaults', icon: 'sync', hint: 'defaults', run: () => $('#reset-dialog').showModal() },
    { group: 'Settings', label: 'Discard unsaved settings', icon: 'x', hint: 'discard', run: discard },
    go('parallel', 'Parallel Execution'), go('models', 'Models & Policy'), go('toml', 'Advanced TOML'),
  ]);

  load();
  buildNav();
  wire();
  const hash = location.hash.slice(1);
  selectSection(SECTIONS.some((s) => s.id === hash) ? hash : lsGet('section', SECTIONS[0].id));

  Store.subscribe((reason) => {
    if (reason === 'project') { load(); selectSection(current); return; }
    if (reason === 'theme') live.forEach((f) => f.sync());
  });
}());
