/* Screen 07 — Reports. Run report KPIs, filterable run history with export, tier-vs-model, scope warnings, reviewer suggestions. */
(function () {
  'use strict';

  const Store = window.OrchDemoStore;
  const UI = window.HerdrUI;
  const { h, iconEl, toast } = UI;
  const $ = (sel, root = document) => root.querySelector(sel);
  const NS = 'http://www.w3.org/2000/svg';
  const REF = new Date('2026-11-18T12:00:00');

  /* ------------------------------------------------------------------ persistence */

  const LS = {
    get(k, f) {
      try { const v = localStorage.getItem(`herdr.reports.${k}`); return v === null ? f : JSON.parse(v); } catch (_) { return f; }
    },
    set(k, v) { try { localStorage.setItem(`herdr.reports.${k}`, JSON.stringify(v)); } catch (_) { /* storage unavailable */ } },
  };

  /* ------------------------------------------------------------------ report history (screen-specific data) */

  const M = (id, title, tier, to, worker, model, fix, rev, fp, status = 'Merged', lost = 0) =>
    ({ id, title, tier, to, worker, model, fix, rev, fp, status, lost });

  const HISTORY = {
    'herdr-orchestra': [
      {
        id: 'run-8f3d2a', title: 'Categories Endpoint & UI', branch: 'feature/categories', status: 'ALL_DONE', done: '2026-11-17T12:08', dur: 2294, cost: 4.86,
        milestones: [
          M('M1', 'DB schema & API contract', 'high', null, 'reviewer', 'opus', 0, 1, true),
          M('M2', 'Categories REST endpoints', 'medium', null, 'coder-1', 'gpt-6-sol', 1, 1, true),
          M('M3', 'Seed data & migration 0007', 'medium', null, 'coder-1', 'gpt-6-sol', 0, 1, true),
          M('M4', 'Navigation & integration wiring', 'medium', 'high', 'coder-2', 'opus', 2, 1, false, 'Merged', 1),
          M('M5', 'Categories list & empty states', 'medium', null, 'coder-2', 'deepseek-v4-pro', 0, 1, true),
          M('M6', 'Edit and delete flows', 'medium', null, 'coder-2', 'deepseek-v4-pro', 0, 1, true),
          M('M7', 'Integration tests', 'high', null, 'coder-1', 'opus', 0, 1, true),
          M('M8', 'Docs and changelog', 'small', null, 'coder-1', 'deepseek-v4-pro', 0, 0, true),
        ],
        scope: [{ ms: 'M2', file: 'src/routes/index.ts', add: 28, del: 6, level: 'warn', detail: 'Modified outside the planned scope for M2 (Categories REST endpoints). No blocker, but review for unintended impact (routing / other modules).' }],
        suggestions: [
          { id: 'S1', ms: 'M2', text: 'Use a shared response type (e.g. ApiResponse<T>) instead of repeating the shape in each handler.' },
          { id: 'S2', ms: 'M7', text: 'Add a minimal integration test for the categories list endpoint (happy path).' },
        ],
      },
      {
        id: 'run-5c1e90', title: 'Session Refresh Tokens', branch: 'feature/session-refresh', status: 'ALL_DONE', done: '2026-11-12T16:41', dur: 3420, cost: 6.12,
        milestones: [
          M('M1', 'Refresh token table & rotation policy', 'high', null, 'coder-1', 'opus', 0, 1, true),
          M('M2', 'Token rotation service', 'high', null, 'coder-1', 'opus', 1, 1, true),
          M('M3', 'Middleware & cookie handling', 'medium', null, 'coder-2', 'gpt-6-sol', 1, 2, false),
          M('M4', 'Revocation endpoint', 'medium', null, 'coder-2', 'gpt-6-sol', 0, 1, true),
          M('M5', 'Tests and docs', 'small', null, 'coder-1', 'haiku', 0, 1, true),
        ],
        scope: [],
        suggestions: [{ id: 'S1', ms: 'M2', text: 'Log the token family id on rotation failures to make incident triage faster.' }],
      },
      {
        id: 'run-a07b44', title: 'Webhook Retry Backoff', branch: 'feature/webhook-retries', status: 'PARTIAL', done: '2026-11-06T09:55', dur: 5280, cost: 8.4,
        milestones: [
          M('M1', 'Retry policy model', 'medium', null, 'coder-1', 'sonnet', 0, 1, true),
          M('M2', 'Exponential backoff scheduler', 'high', null, 'coder-2', 'opus', 1, 1, true),
          M('M3', 'Dead-letter queue', 'high', null, 'coder-2', 'opus', 3, 2, false, 'Failed', 2),
          M('M4', 'Delivery dashboard', 'medium', null, null, 'sonnet', 0, 0, false, 'Held'),
        ],
        scope: [
          { ms: 'M2', file: 'src/queue/worker.rs', add: 41, del: 12, level: 'warn', detail: 'Worker loop touched while adding the scheduler. Confirm the dead-letter hand-off was not changed.' },
          { ms: 'M3', file: 'migrations/0012_dead_letters.sql', add: 18, del: 0, level: 'warn', detail: 'Migration added outside the likely files for M3. Held by the risk gate and approved manually.' },
        ],
        suggestions: [],
      },
      {
        id: 'run-31d9ce', title: 'CSV Export Endpoint', branch: 'feature/csv-export', status: 'FAILED', done: '2026-10-21T18:20', dur: 1860, cost: 2.35,
        milestones: [
          M('M1', 'Streaming CSV writer', 'medium', null, 'coder-1', 'sonnet', 3, 2, false, 'Failed', 3),
          M('M2', 'Export endpoint and auth', 'medium', null, null, 'sonnet', 0, 0, false, 'Held'),
        ],
        scope: [],
        suggestions: [],
      },
      {
        id: 'run-2be6f1', title: 'Audit Log Viewer', branch: 'feature/audit-log', status: 'ALL_DONE', done: '2026-09-30T14:02', dur: 4140, cost: 5.7,
        milestones: [
          M('M1', 'Audit event schema', 'high', null, 'coder-1', 'opus', 0, 1, true),
          M('M2', 'Query API with filters', 'medium', null, 'coder-2', 'gpt-6-sol', 0, 1, true),
          M('M3', 'Viewer table and pagination', 'medium', null, 'coder-2', 'deepseek-v4-pro', 1, 1, true),
          M('M4', 'Export and docs', 'small', null, 'coder-1', 'haiku', 0, 1, true),
        ],
        scope: [],
        suggestions: [],
      },
    ],
    streamforge: [
      {
        id: 'run-7a20d1', title: 'Schema Registry Cache', branch: 'feature/registry-cache', status: 'ALL_DONE', done: '2026-11-15T20:31', dur: 3180, cost: 5.25,
        milestones: [
          M('M1', 'LRU cache for schema lookups', 'medium', null, 'coder-1', 'sonnet', 0, 1, true),
          M('M2', 'Invalidation on registry update', 'high', null, 'coder-1', 'opus', 1, 1, true),
          M('M3', 'Cache hit-rate metrics', 'medium', null, 'coder-1', 'sonnet', 0, 1, true),
        ],
        scope: [{ ms: 'M2', file: 'crates/ingest/src/pipeline.rs', add: 9, del: 3, level: 'warn', detail: 'Pipeline entry point touched to thread the invalidation handle. Small, but outside the likely files for M2.' }],
        suggestions: [{ id: 'S1', ms: 'M3', text: 'Expose cache capacity as a config value instead of a constant.' }],
      },
      {
        id: 'run-9c4e3b', title: 'Consumer Lag Alerts', branch: 'feature/lag-alerts', status: 'PARTIAL', done: '2026-11-09T11:12', dur: 4620, cost: 6.9,
        milestones: [
          M('M1', 'Lag gauge per partition', 'medium', null, 'coder-1', 'sonnet', 0, 1, true),
          M('M2', 'Alert rule templates', 'medium', 'high', 'coder-1', 'opus', 2, 2, false, 'Failed', 1),
        ],
        scope: [],
        suggestions: [],
      },
      {
        id: 'run-1f88aa', title: 'Ingest Batching', branch: 'feature/ingest-batching', status: 'ALL_DONE', done: '2026-10-12T15:47', dur: 2700, cost: 3.8,
        milestones: [
          M('M1', 'Batch assembler', 'medium', null, 'coder-1', 'sonnet', 0, 1, true),
          M('M2', 'Flush timer and size limit', 'medium', null, 'coder-1', 'sonnet', 0, 1, true),
          M('M3', 'Benchmarks', 'small', null, 'coder-1', 'haiku', 0, 0, true),
        ],
        scope: [],
        suggestions: [],
      },
    ],
  };

  /* The in-flight run comes from the live store so it follows the current project. */
  function liveRun() {
    const d = Store.data();
    const stateLabel = { merged: 'Merged', held: 'Held', failed: 'Failed' };
    const milestones = d.plan.milestones.map((pm) => {
      const t = d.tasks.find((x) => x.milestone === pm.id) || { state: 'pending', attempts: 0, agent: null };
      const ag = d.agents.find((a) => a.id === t.agent);
      const model = ag ? ag.model : 'sonnet';
      return M(pm.id, pm.title, pm.tier, null, t.agent, model, Math.max(0, t.attempts - 1), t.state === 'review' ? 1 : 0,
        t.state === 'merged' && t.attempts <= 1, stateLabel[t.state] || 'In progress', t.state === 'failed' ? t.attempts : 0);
    });
    return {
      id: d.report.run_id, title: d.plan.title, branch: d.status.branch, status: 'RUNNING', done: null,
      dur: d.report.duration_s, cost: d.report.cost.usd, milestones, scope: [], suggestions: [],
      live: true, tokens: d.report.cost,
    };
  }

  const runsFor = () => [...(HISTORY[Store.project] || []).map((r) => ({ ...r, project: Store.project })), { ...liveRun(), project: Store.project }];

  /* ------------------------------------------------------------------ derived stats + formatting */

  const STATUS_TONE = { ALL_DONE: 'emerald', PARTIAL: 'amber', FAILED: 'coral', RUNNING: 'cyan' };
  const TIER_TONE = { high: 'coral', medium: 'amber', small: 'cyan' };
  const MS_TONE = { Merged: 'emerald', Failed: 'coral', Held: 'amber', 'In progress': 'cyan' };

  function stats(run) {
    const ms = run.milestones;
    const pass = ms.filter((m) => m.fp).length;
    const esc = ms.filter((m) => m.to);
    return {
      pass, total: ms.length, pct: ms.length ? Math.round((pass / ms.length) * 1000) / 10 : 0,
      escalations: esc.length,
      escNote: esc.length ? esc.map((m) => `${m.id} escalated ${m.tier} → ${m.to}`).join(', ') : 'No tier escalations',
      lost: ms.reduce((n, m) => n + m.lost, 0),
    };
  }

  const pctText = (n) => `${Number.isInteger(n) ? n : n.toFixed(1)}%`;

  function fmtDur(s) {
    const hh = Math.floor(s / 3600);
    const mm = Math.floor((s % 3600) / 60);
    if (hh) return `${hh}h ${String(mm).padStart(2, '0')}m`;
    return `${mm}m ${String(s % 60).padStart(2, '0')}s`;
  }

  const fmtDate = (iso) => (iso
    ? new Date(iso).toLocaleString('en-US', { month: 'short', day: 'numeric', year: 'numeric', hour: 'numeric', minute: '2-digit' })
    : 'In progress');
  const fmtDay = (iso) => (iso ? new Date(iso).toLocaleDateString('en-US', { month: 'short', day: 'numeric' }) : 'Live');
  const usd = (n) => `$${n.toFixed(2)}`;
  const badge = (tone, cls, ...kids) => h('span', { class: `badge ${tone && tone !== 'neutral' ? `badge--${tone}` : ''} ${cls || ''}`.trim() }, ...kids);

  /* ------------------------------------------------------------------ state */

  const state = { runId: null, q: '', range: 'all', status: 'all', sort: 'date-desc', mf: 'all', scopeSel: 0 };

  function currentRun() {
    const rs = runsFor();
    return rs.find((r) => r.id === state.runId) || rs[0];
  }

  function pickDefaultRun() {
    const rs = runsFor();
    const saved = LS.get(`run.${Store.project}`, null);
    state.runId = (rs.find((r) => r.id === saved) || rs.find((r) => !r.live) || rs[0]).id;
    state.scopeSel = 0;
  }

  const prs = () => LS.get('prs', {});
  const queued = () => LS.get('queued', {});

  /* ------------------------------------------------------------------ history: filter, sort, table, chart */

  const SORTS = {
    'date-desc': (a, b) => (b.done || '9').localeCompare(a.done || '9'),
    'date-asc': (a, b) => (a.done || '9').localeCompare(b.done || '9'),
    'fp-desc': (a, b) => stats(b).pct - stats(a).pct,
    'fp-asc': (a, b) => stats(a).pct - stats(b).pct,
    'dur-desc': (a, b) => b.dur - a.dur,
    'cost-desc': (a, b) => b.cost - a.cost,
    'plan-asc': (a, b) => a.title.localeCompare(b.title),
  };

  function visibleRuns() {
    const q = state.q.trim().toLowerCase();
    return runsFor()
      .filter((r) => !q || `${r.id} ${r.title} ${r.branch}`.toLowerCase().includes(q))
      .filter((r) => state.status === 'all' || r.status === state.status)
      .filter((r) => state.range === 'all' || r.live || (REF - new Date(r.done)) / 86400000 <= Number(state.range))
      .sort(SORTS[state.sort]);
  }

  const HEADS = [
    { label: 'Run' },
    { label: 'Plan' },
    { label: 'Completed', asc: 'date-asc', desc: 'date-desc' },
    { label: 'Duration', desc: 'dur-desc' },
    { label: 'First-pass', asc: 'fp-asc', desc: 'fp-desc' },
    { label: 'Cost', desc: 'cost-desc' },
    { label: 'Status' },
  ];

  function renderHistoryHead() {
    const tr = h('tr', null, HEADS.map((c) => {
      const sorts = [c.asc, c.desc].filter(Boolean);
      const active = sorts.includes(state.sort);
      const th = h('th', { scope: 'col', 'aria-sort': active ? (state.sort.endsWith('asc') ? 'ascending' : 'descending') : null });
      if (!sorts.length) { th.textContent = c.label; return th; }
      const next = active && c.desc && state.sort === c.desc && c.asc ? c.asc : (c.desc || c.asc);
      th.append(h('button', {
        type: 'button', class: 'thsort', dataset: { fk: `sort-${c.label}` },
        'aria-label': `Sort by ${c.label}`,
        on: { click: () => { state.sort = next; $('#f-sort').value = next; renderHistory(); } },
      }, c.label, active ? h('span', { 'aria-hidden': 'true' }, state.sort.endsWith('asc') ? ' ▲' : ' ▼') : null));
      return th;
    }));
    $('#hist-table thead').replaceChildren(tr);
  }

  function selectRun(id) {
    state.runId = id;
    state.scopeSel = 0;
    LS.set(`run.${Store.project}`, id);
    UI.preserveFocus(() => { renderHistory(); renderRun(); });
  }

  function renderHistory() {
    const rs = visibleRuns();
    const total = runsFor().length;
    $('#hist-count').textContent = `${rs.length} of ${total} runs`;
    renderHistoryHead();
    const body = $('#hist-table tbody');
    if (!rs.length) {
      body.replaceChildren(h('tr', null, h('td', { colspan: 7, class: 'empty' }, 'No runs match these filters. Clear the filters to see the full history.')));
    } else {
      body.replaceChildren(...rs.map((r) => {
        const s = stats(r);
        const cur = r.id === state.runId;
        return h('tr', { class: cur ? 'is-current' : null, dataset: { run: r.id } },
          h('td', null, h('button', {
            type: 'button', class: 'idbtn mono', dataset: { fk: `run-${r.id}` }, 'aria-current': cur ? 'true' : null,
            'aria-label': `Open report ${r.id}`, on: { click: () => selectRun(r.id) },
          }, r.id)),
          h('td', null, h('span', { class: 'plan-name' }, r.title), h('span', { class: 'mono sub' }, r.branch)),
          h('td', { class: 'nowrap' }, fmtDate(r.done)),
          h('td', { class: 'mono nowrap' }, fmtDur(r.dur)),
          h('td', { class: 'mono nowrap' }, `${pctText(s.pct)} (${s.pass}/${s.total})`),
          h('td', { class: 'mono nowrap' }, usd(r.cost)),
          h('td', null, badge(STATUS_TONE[r.status], 'badge--mono', r.status)));
      }));
    }
    renderChart(rs);
  }

  function svgEl(tag, attrs, ...kids) {
    const n = document.createElementNS(NS, tag);
    Object.entries(attrs || {}).forEach(([k, v]) => { if (v !== null && v !== undefined) n.setAttribute(k, v); });
    kids.forEach((k) => n.append(k));
    return n;
  }

  function renderChart(rs) {
    const host = $('#chart');
    const data = [...rs].sort(SORTS['date-asc']);
    if (!data.length) { host.replaceChildren(h('p', { class: 'empty' }, 'No runs to chart.')); return; }
    const W = 520; const H = 210; const L = 34; const B = 26; const T = 18; const R = 8;
    const innerW = W - L - R; const innerH = H - T - B;
    const step = innerW / data.length;
    const bw = Math.min(46, step * 0.62);
    const svg = svgEl('svg', {
      viewBox: `0 0 ${W} ${H}`, role: 'group', 'aria-label': `First-pass rate for ${data.length} run${data.length === 1 ? '' : 's'}. The run history table lists the same values.`,
      class: 'chart__svg',
    });
    [0, 50, 100].forEach((v) => {
      const y = T + innerH - (v / 100) * innerH;
      svg.append(svgEl('line', { x1: L, x2: W - R, y1: y, y2: y, class: 'ch-grid' }));
      const t = svgEl('text', { x: L - 6, y: y + 3, 'text-anchor': 'end', class: 'ch-axis' });
      t.textContent = `${v}%`;
      svg.append(t);
    });
    data.forEach((r, i) => {
      const s = stats(r);
      const x = L + step * i + (step - bw) / 2;
      const bh = Math.max(2, (s.pct / 100) * innerH);
      const y = T + innerH - bh;
      const tone = r.id === state.runId ? 'sel' : r.status === 'FAILED' ? 'bad' : r.status === 'PARTIAL' ? 'warn' : 'ok';
      const label = `${r.id}, ${r.title}: first-pass ${pctText(s.pct)}, cost ${usd(r.cost)}, ${r.status}`;
      const g = svgEl('g', {
        class: `ch-bar ch-bar--${tone}`, tabindex: '0', role: 'button', 'aria-label': label,
        'data-fk': `bar-${r.id}`,
      }, svgEl('title', null, document.createTextNode(label)), svgEl('rect', { x, y, width: bw, height: bh, rx: 3 }));
      const val = svgEl('text', { x: x + bw / 2, y: y - 4, 'text-anchor': 'middle', class: 'ch-val' });
      val.textContent = pctText(s.pct);
      const lab = svgEl('text', { x: x + bw / 2, y: H - 8, 'text-anchor': 'middle', class: 'ch-axis' });
      lab.textContent = r.live ? 'live' : fmtDay(r.done);
      g.append(val, lab);
      g.addEventListener('click', () => selectRun(r.id));
      g.addEventListener('keydown', (e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); selectRun(r.id); } });
      svg.append(g);
    });
    host.replaceChildren(svg);
  }

  /* ------------------------------------------------------------------ export */

  function download(name, text, type) {
    const blob = new Blob([text], { type });
    const url = URL.createObjectURL(blob);
    const a = h('a', { href: url, download: name, hidden: true });
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }

  const csvCell = (v) => { const s = String(v ?? ''); return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s; };

  function runRow(r) {
    const s = stats(r);
    return {
      run_id: r.id, project: Store.project, plan: r.title, branch: r.branch, status: r.status, completed_at: r.done || '',
      duration_s: r.dur, first_pass_pct: s.pct, first_pass: `${s.pass}/${s.total}`, escalations: s.escalations, lost_attempts: s.lost, cost_usd: r.cost,
    };
  }

  function exportVisible(kind) {
    const rs = visibleRuns();
    if (!rs.length) { toast('Nothing to export. Clear the filters first.', 'amber', 'warning'); return; }
    const rows = rs.map(runRow);
    if (kind === 'csv') {
      const cols = Object.keys(rows[0]);
      download(`herdr-runs-${Store.project}.csv`, [cols.join(','), ...rows.map((r) => cols.map((c) => csvCell(r[c])).join(','))].join('\n') + '\n', 'text/csv');
    } else {
      download(`herdr-runs-${Store.project}.json`, JSON.stringify(rows, null, 2) + '\n', 'application/json');
    }
    toast(`Exported ${rows.length} run${rows.length === 1 ? '' : 's'} as ${kind.toUpperCase()}`, 'emerald', 'check');
  }

  function exportRunJson() {
    const r = currentRun();
    const payload = { ...runRow(r), milestones: r.milestones, scope_warnings: r.scope, reviewer_suggestions: r.suggestions };
    download(`${r.id}.json`, JSON.stringify(payload, null, 2) + '\n', 'application/json');
    toast(`Exported ${r.id}.json`, 'emerald', 'check');
  }

  /* ------------------------------------------------------------------ selected run: head + KPIs */

  function renderHead(r, s) {
    $('#rep-icon').replaceChildren(iconEl('doc'));
    $('#rep-title').textContent = `Run Report: ${r.title}`;
    const st = $('#rep-status');
    st.className = `badge badge--mono badge--${STATUS_TONE[r.status]}`;
    st.textContent = r.status;
    $('#rep-meta').replaceChildren(
      'Run ID: ', h('span', { class: 'mono' }, r.id), ' · Project: ', h('span', { class: 'mono' }, Store.project),
      ' · Branch: ', h('span', { class: 'mono' }, r.branch), ` · ${r.live ? 'Started' : 'Completed'}: ${r.live ? fmtDate(Store.data().report.started_at) : fmtDate(r.done)}`);
    const pr = prs()[r.id];
    const fin = $('#finish-btn');
    fin.replaceChildren(iconEl('branch'), pr ? `PR #${pr} created` : 'Finish & Create PR');
    fin.disabled = Boolean(pr) || r.status !== 'ALL_DONE';
    fin.title = pr ? `Pull request #${pr} already exists` : r.status !== 'ALL_DONE' ? 'Only runs with every milestone merged can be finished' : '';
    $('#export-run-btn').replaceChildren(iconEl('external'), 'Export JSON');
    $('#summary-btn').replaceChildren(iconEl('doc'), 'View summary.md');
    $('#hist-title').replaceChildren(iconEl('clock'), 'Run History');
  }

  function renderKpis(r, s) {
    const tiles = [
      { icon: 'checkcircle', tone: 'emerald', title: 'First-Pass Rate', deco: 'chart', big: pctText(s.pct), sub: `${s.pass} / ${s.total} milestones passed check & review on round 1` },
      { icon: 'clock', tone: 'cyan', title: 'Total Execution Time', deco: 'clock', big: fmtDur(r.dur), sub: 'Active manager time · Idle excluded' },
      { icon: 'bolt', tone: 'violet', title: 'Escalations Used', deco: 'external', big: String(s.escalations), sub: s.escNote },
      { icon: 'layers', tone: 'coral', title: 'Wasted Work', deco: 'trash', big: `${s.lost} lost attempt${s.lost === 1 ? '' : 's'}`, sub: `${r.discarded || 0} discarded commits` },
    ];
    $('#kpis').replaceChildren(...tiles.map((t) => h('article', { class: `card kpi tone-${t.tone}` },
      h('div', { class: 'kpi__top' }, h('span', { class: 'kpi__icon' }, iconEl(t.icon)), h('h2', { class: 'kpi__title' }, t.title), h('span', { class: 'kpi__deco', 'aria-hidden': 'true' }, iconEl(t.deco))),
      h('p', { class: 'kpi__big' }, t.big),
      h('p', { class: 'kpi__sub' }, t.sub))));
  }

  /* ------------------------------------------------------------------ selected run: tier table, models, scope, suggestions */

  function openMilestone(r, m) {
    $('#ms-title').replaceChildren(h('span', { class: 'mono' }, m.id), ` · ${m.title}`);
    const row = (k, v) => h('div', { class: 'kv' }, h('span', { class: 'muted' }, k), v);
    $('#ms-body').replaceChildren(
      row('Planned tier', badge(TIER_TONE[m.tier], '', m.to ? `${m.tier} → ${m.to}` : m.tier)),
      row('Worker', h('span', { class: 'mono text-violet' }, m.worker || 'unassigned')),
      row('Actual model', badge('violet', 'badge--mono', m.model)),
      row('Fix rounds / review rounds', h('span', { class: 'mono' }, `${m.fix} / ${m.rev}`)),
      row('First pass', h('span', null, m.fp ? 'Yes, check and review passed on round 1' : 'No, needed rework or did not finish')),
      row('Lost attempts', h('span', { class: 'mono' }, String(m.lost))),
      row('Status', badge(MS_TONE[m.status], '', m.status)),
      row('Branch', h('span', { class: 'mono' }, `orch/${m.id.toLowerCase()}-${m.title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 28)}`)));
    $('#ms-dialog').showModal();
  }

  function renderTiers(r) {
    $('#tier-title').replaceChildren(iconEl('layers'), 'Planned Tier vs. Actual Model (Milestones)');
    const ms = r.milestones.filter((m) => state.mf === 'all' || (state.mf === 'rework' ? m.fix > 0 : Boolean(m.to)));
    $('#tier-count').textContent = `${ms.length} milestone${ms.length === 1 ? '' : 's'}`;
    $('#m-filter').querySelectorAll('button').forEach((b) => b.setAttribute('aria-pressed', String(b.dataset.v === state.mf)));
    $('#tier-table tbody').replaceChildren(...(ms.length ? ms.map((m, i) => h('tr', null,
      h('td', { class: 'mono muted' }, String(i + 1).padStart(2, '0')),
      h('td', { class: 'ms-cell' },
        h('button', { type: 'button', class: 'idbtn mono', dataset: { fk: `ms-${m.id}` }, 'aria-label': `Details for ${m.id}, ${m.title}`, on: { click: () => openMilestone(r, m) } }, m.id),
        h('span', null, m.title)),
      h('td', null, h('span', { class: 'tierflow' }, badge(TIER_TONE[m.tier], 'badge--mono', m.tier), m.to ? [h('span', { class: 'muted', 'aria-label': 'escalated to' }, '→'), badge(TIER_TONE[m.to], 'badge--mono', m.to)] : null)),
      h('td', { class: 'mono text-violet' }, m.worker || '—'),
      h('td', null, badge('violet', 'badge--mono', m.model)),
      h('td', { class: 'mono nowrap' }, `${m.fix} / ${m.rev}`),
      h('td', null, badge(MS_TONE[m.status], '', m.status === 'Merged' ? iconEl('checkcircle', 12) : null, m.status))))
      : [h('tr', null, h('td', { colspan: 7, class: 'empty' }, 'No milestones match this filter.'))]));
  }

  function renderModels(r) {
    $('#model-title').replaceChildren(iconEl('chart'), 'Per-Model Performance Breakdown');
    const by = new Map();
    r.milestones.forEach((m) => {
      const e = by.get(m.model) || { model: m.model, pass: 0, total: 0 };
      e.total += 1; if (m.fp) e.pass += 1;
      by.set(m.model, e);
    });
    const rows = [...by.values()].sort((a, b) => b.total - a.total);
    $('#models').replaceChildren(
      h('div', { class: 'mrow mrow--head' }, h('span', null, 'Model'), h('span', null, 'First-pass rate (milestones)')),
      ...rows.map((e) => {
        const pct = Math.round((e.pass / e.total) * 100);
        const tone = pct >= 85 ? 'emerald' : pct >= 60 ? 'amber' : 'coral';
        return h('div', { class: 'mrow' },
          badge('violet', 'badge--mono', e.model),
          h('div', { class: 'mbar' },
            h('span', { class: 'mbar__lab' }, `${pct}% `, h('span', { class: 'muted' }, `(${e.pass} / ${e.total})`)),
            h('div', { class: `bar tone-${tone}`, role: 'img', 'aria-label': `${e.model}: ${pct} percent first-pass, ${e.pass} of ${e.total} milestones` }, h('i', { style: `width:${pct}%` }))),
          h('span', { class: 'mono mpct' }, `${pct}%`));
      }));
  }

  function renderScope(r) {
    $('#scope-title').replaceChildren(iconEl('warning'), 'Scope Warnings (Files Outside Likely Files)');
    $('#scope-count').textContent = `${r.scope.length} item${r.scope.length === 1 ? '' : 's'}`;
    const body = $('#scope-table tbody');
    const det = $('#scope-details');
    if (!r.scope.length) {
      body.replaceChildren(h('tr', null, h('td', { colspan: 5, class: 'empty' }, 'Every modified file was inside the planned scope.')));
      det.hidden = true;
      return;
    }
    state.scopeSel = Math.min(state.scopeSel, r.scope.length - 1);
    body.replaceChildren(...r.scope.map((w, i) => h('tr', { class: i === state.scopeSel ? 'is-current' : null },
      h('td', { class: 'mono muted' }, String(i + 1).padStart(2, '0')),
      h('td', null, h('span', { class: 'idchip mono' }, w.ms)),
      h('td', null, h('button', {
        type: 'button', class: 'path-chip filebtn', dataset: { fk: `scope-${i}` }, 'aria-pressed': String(i === state.scopeSel),
        on: { click: () => { state.scopeSel = i; UI.preserveFocus(() => renderScope(currentRun())); } },
      }, w.file)),
      h('td', { class: 'mono nowrap' }, h('span', { class: 'text-emerald' }, `+${w.add}`), ' ', h('span', { class: 'text-coral' }, `−${w.del}`)),
      h('td', null, badge('amber', 'badge--mono', w.level)))));
    const sel = r.scope[state.scopeSel];
    det.hidden = false;
    det.replaceChildren(iconEl('help'), h('div', null, h('strong', null, 'Details'), h('p', null, sel.detail)));
  }

  function fixCommand(r, sg) {
    const text = sg.text.replace(/"/g, "'");
    return `orch fix --milestone ${sg.ms} --from-report ${r.id} "${text}"`;
  }

  let fixTarget = null;

  function renderSuggestions(r) {
    $('#sugg-title').replaceChildren(iconEl('chat'), 'Unapplied Reviewer Suggestions');
    $('#sugg-count').textContent = `${r.suggestions.length} suggestion${r.suggestions.length === 1 ? '' : 's'}`;
    const q = queued();
    const list = $('#sugg');
    if (!r.suggestions.length) { list.replaceChildren(h('li', { class: 'empty' }, 'The reviewer left no unapplied suggestions for this run.')); return; }
    list.replaceChildren(...r.suggestions.map((sg) => {
      const key = `${r.id}:${sg.id}`;
      const done = Boolean(q[key]);
      return h('li', { class: 'sg' },
        h('span', { class: 'dot dot--violet', 'aria-hidden': 'true' }),
        h('span', { class: 'idchip mono', title: `Milestone ${sg.ms}` }, sg.id),
        h('p', { class: 'sg__text' }, sg.text),
        h('button', {
          type: 'button', class: 'btn btn--sm sg__btn', disabled: done, 'aria-haspopup': 'dialog', dataset: { fk: `fix-${sg.id}` },
          on: { click: () => { fixTarget = { run: r, sg }; openFix(); } },
        }, iconEl(done ? 'check' : 'branch'), done ? 'Queued (orch fix)' : 'Create Follow-Up Fix (orch fix)'));
    }));
  }

  function openFix() {
    const { run, sg } = fixTarget;
    $('#fix-text').textContent = `Queue a follow-up fix for suggestion ${sg.id} on milestone ${sg.ms}. The lead runs this as a new milestone on ${run.branch}.`;
    $('#fix-cmd').textContent = fixCommand(run, sg);
    $('#fix-dialog').showModal();
  }

  function renderRun() {
    const r = currentRun();
    const s = stats(r);
    renderHead(r, s);
    renderKpis(r, s);
    renderTiers(r);
    renderModels(r);
    renderScope(r);
    renderSuggestions(r);
  }

  /* ------------------------------------------------------------------ summary.md */

  function summaryText(r) {
    const s = stats(r);
    const lines = [
      `# Run Report: ${r.title}`, '',
      `- Run: ${r.id}`, `- Project: ${Store.project}`, `- Branch: ${r.branch}`, `- Status: ${r.status}`, `- ${r.live ? 'Started' : 'Completed'}: ${r.live ? fmtDate(Store.data().report.started_at) : fmtDate(r.done)}`,
      '', '## Metrics', '',
      `- First-pass rate: ${pctText(s.pct)} (${s.pass}/${s.total} milestones)`,
      `- Execution time: ${fmtDur(r.dur)} (active manager time, idle excluded)`,
      `- Escalations used: ${s.escalations}${s.escalations ? ` (${s.escNote})` : ''}`,
      `- Wasted work: ${s.lost} lost attempt${s.lost === 1 ? '' : 's'}, ${r.discarded || 0} discarded commits`,
      `- Cost: ${usd(r.cost)}`,
      '', '## Milestones', '',
      ...r.milestones.map((m) => `- ${m.id} ${m.title} [${m.tier}${m.to ? ` -> ${m.to}` : ''}] ${m.worker || 'unassigned'} / ${m.model}, rounds ${m.fix}/${m.rev}, ${m.status}`),
    ];
    if (r.scope.length) lines.push('', '## Scope warnings', '', ...r.scope.map((w) => `- ${w.ms} ${w.file} (+${w.add} -${w.del}) ${w.level}`));
    if (r.suggestions.length) lines.push('', '## Unapplied reviewer suggestions', '', ...r.suggestions.map((g) => `- ${g.id} (${g.ms}): ${g.text}`));
    return `${lines.join('\n')}\n`;
  }

  async function copyText(text, label) {
    try { await navigator.clipboard.writeText(text); toast(`Copied ${label}`, 'cyan', 'copy'); }
    catch (_) { toast('Copy is blocked in this context. Select the text and copy it manually.', 'amber', 'warning'); }
  }

  /* ------------------------------------------------------------------ wiring */

  function resetFilters() {
    Object.assign(state, { q: '', range: 'all', status: 'all', sort: 'date-desc' });
    $('#f-q').value = ''; $('#f-range').value = 'all'; $('#f-status').value = 'all'; $('#f-sort').value = 'date-desc';
    renderHistory();
  }

  function wire() {
    $('#f-q').addEventListener('input', (e) => { state.q = e.target.value; renderHistory(); });
    $('#f-range').addEventListener('change', (e) => { state.range = e.target.value; renderHistory(); });
    $('#f-status').addEventListener('change', (e) => { state.status = e.target.value; renderHistory(); });
    $('#f-sort').addEventListener('change', (e) => { state.sort = e.target.value; renderHistory(); });
    $('#f-reset').replaceChildren(iconEl('sync'), 'Reset filters');
    $('#f-reset').addEventListener('click', resetFilters);
    $('#exp-csv').replaceChildren(iconEl('external'), 'Export CSV');
    $('#exp-json').replaceChildren(iconEl('external'), 'Export JSON');
    $('#exp-csv').addEventListener('click', () => exportVisible('csv'));
    $('#exp-json').addEventListener('click', () => exportVisible('json'));
    $('#export-run-btn').addEventListener('click', exportRunJson);

    $('#m-filter').addEventListener('click', (e) => {
      const b = e.target.closest('button[data-v]');
      if (!b) return;
      state.mf = b.dataset.v;
      renderTiers(currentRun());
    });

    $('#summary-btn').addEventListener('click', () => {
      $('#summary-text').textContent = summaryText(currentRun());
      $('#summary-dialog').showModal();
    });
    $('#summary-copy').replaceChildren(iconEl('copy'), 'Copy');
    $('#summary-dl').replaceChildren(iconEl('external'), 'Download');
    $('#summary-copy').addEventListener('click', () => copyText($('#summary-text').textContent, 'summary.md'));
    $('#summary-dl').addEventListener('click', () => { download(`${currentRun().id}-summary.md`, $('#summary-text').textContent, 'text/markdown'); toast('Downloaded summary.md', 'emerald', 'check'); });
    $('#summary-close').addEventListener('click', () => $('#summary-dialog').close());
    $('#ms-close').addEventListener('click', () => $('#ms-dialog').close());

    $('#finish-btn').addEventListener('click', () => {
      const r = currentRun();
      $('#finish-lead').replaceChildren(`Open a pull request from `, h('span', { class: 'mono' }, r.branch), ' into ', h('span', { class: 'mono' }, 'main'), '. All milestones are merged and verified.');
      $('#pr-title').value = r.title;
      $('#finish-dialog').showModal();
    });
    $('#finish-dialog').addEventListener('close', (e) => {
      if (e.target.returnValue !== 'create') return;
      const r = currentRun();
      const n = 480 + Math.abs([...r.id].reduce((a, c) => a + c.charCodeAt(0), 0)) % 40;
      const all = prs(); all[r.id] = n; LS.set('prs', all);
      renderRun();
      toast(`Pull request #${n} created for ${r.branch}`, 'emerald', 'branch');
    });

    $('#fix-copy').replaceChildren(iconEl('copy'), 'Copy command');
    $('#fix-copy').addEventListener('click', () => copyText($('#fix-cmd').textContent, 'command'));
    $('#fix-dialog').addEventListener('close', (e) => {
      if (e.target.returnValue !== 'queue' || !fixTarget) return;
      const q = queued(); q[`${fixTarget.run.id}:${fixTarget.sg.id}`] = true; LS.set('queued', q);
      renderSuggestions(currentRun());
      toast(`Follow-up fix queued for ${fixTarget.sg.id}`, 'violet', 'branch');
    });
    ['#summary-dialog', '#finish-dialog', '#fix-dialog', '#ms-dialog'].forEach((s) => UI.enhanceDialog($(s)));
  }

  window.HerdrShell.registerCommands([
    { group: 'Reports', label: 'Export visible runs as CSV', icon: 'external', hint: 'csv', run: () => exportVisible('csv') },
    { group: 'Reports', label: 'Export visible runs as JSON', icon: 'external', hint: 'json', run: () => exportVisible('json') },
    { group: 'Reports', label: 'View summary.md for selected run', icon: 'doc', hint: 'summary', run: () => $('#summary-btn').click() },
    { group: 'Reports', label: 'Show only failed runs', icon: 'warning', hint: 'FAILED', run: () => { state.status = 'FAILED'; $('#f-status').value = 'FAILED'; renderHistory(); } },
    { group: 'Reports', label: 'Reset report filters', icon: 'sync', hint: 'filters', run: resetFilters },
  ]);

  wire();
  pickDefaultRun();
  renderHistory();
  renderRun();
  Store.subscribe((reason) => {
    if (reason === 'theme') return;
    if (reason === 'project') { resetFilters(); pickDefaultRun(); renderHistory(); renderRun(); return; }
    UI.preserveFocus(() => { renderHistory(); renderRun(); });
  });
}());
