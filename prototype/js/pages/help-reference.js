/* Screen 11 — Help & Reference. Static reference content with live search, accordions, copy buttons
   and working keyboard shortcuts (g then c/m/p, Shift+A, /). */
(function () {
  'use strict';

  const UI = window.HerdrUI;
  const { h, iconEl, toast } = UI;
  const $ = (sel, root = document) => root.querySelector(sel);

  /* ------------------------------------------------------------------ content */

  const CLI = [
    { id: 'lead', title: 'Lead & Plan', sub: 'Create plans, sync context and start orchestration.', icon: 'folder', rows: [
      ['orch lead plan', '# Generate a plan from goal'],
      ['orch sync', '# Sync state, agents and skills'],
      ['orch start', '# Start the planned run'],
      ['orch dispatch M1 M2', '# Dispatch specific milestones'],
    ] },
    { id: 'control', title: 'Control', sub: 'Control, approve, fix and escalate a running run.', icon: 'play', rows: [
      ['orch pause', '# Pause the current run'],
      ['orch resume', '# Resume a paused run'],
      ['orch approve M4', '# Approve a held milestone (M4)'],
      ['orch fix M2 "text"', '# Create a follow-up fix for M2'],
      ['orch escalate M3', '# Escalate M3 (medium → high)'],
    ] },
    { id: 'diag', title: 'Diagnostics', sub: 'Check health, view events and finish a run.', icon: 'activity', rows: [
      ['orch doctor', '# Run full environment & setup check'],
      ['orch status', '# Show current run and agent status'],
      ['orch events --peek', '# Peek at recent events (last 100)'],
      ['orch finish', '# Finalize run and generate report'],
    ] },
  ];

  const EXIT = [
    [0, 'All Done', 'Run completed successfully (all milestones merged).'],
    [2, 'Rule Refused', 'Blocked by a guardrail (e.g., policy, scope, or risk rule).'],
    [3, 'Needs Human / Retry Cap', 'Requires human input or retry cap reached.'],
    [4, 'Merge Conflict', 'Git merge conflict needs manual resolution.'],
    [6, 'Check Failed', 'Tests or verification failed (e.g., cargo test / npm test).'],
    [7, 'Held for Approval', 'Waiting for explicit human approval (risk hold, large change).'],
    [10, 'Poll Wait', 'No actionable event yet (keep polling / watching).'],
  ];

  const ENV = [
    ['ORCH_CODER_BUDGET_MIN', '45'],
    ['ORCH_LEAD_WAIT_MIN', '15'],
    ['ORCH_RISK_MAX_LINES', '1500'],
  ];

  const KEYS = [
    { keys: [['Ctrl'], ['K']], joiner: '+', title: 'Open Command Palette', sub: '(run commands, search, quick actions)' },
    { keys: [['G'], ['C']], joiner: 'then', title: 'Go to Chat / Home', sub: '(agent chat and quick prompts)' },
    { keys: [['G'], ['M']], joiner: 'then', title: 'Go to Execution Monitor', sub: '(live run, agent sessions, verification)' },
    { keys: [['G'], ['P']], joiner: 'then', title: 'Go to Plan', sub: '(view and edit the orchestration plan)' },
    { keys: [['Shift'], ['A']], joiner: '+', title: 'Open Attention Inbox', sub: '(questions, risk holds, blocked prompts)' },
    { keys: [['/']], joiner: '', title: 'Search this page', sub: '(focus the help search box)' },
  ];

  const GUARANTEES = [
    ['Single Driver Lease', 'Only one active lead/driver controls orchestration at a time.'],
    ['SQLite Transactions', 'Durable, ordered state for plans, runs and events.'],
    ['Hidden Git Refs (refs/orch/*)', 'Orchestration metadata kept in isolated Git refs (no branch pollution).'],
    ['Zero Embedded Terminals', 'No in-app terminal emulators. All execution is via external agent CLIs (e.g., herdr, claude, codex).'],
  ];

  /* ------------------------------------------------------------------ helpers */

  const open = new Set(CLI.map((g) => g.id));
  let q = '';
  const norm = (s) => String(s).toLowerCase();
  const hit = (...parts) => !q || norm(parts.join(' ')).includes(q);
  let shownCount = 0;

  function copyText(text, btn) {
    const ok = () => {
      toast(`Copied: ${text}`, 'emerald', 'copy');
      if (btn) { btn.dataset.copied = 'true'; setTimeout(() => { if (btn.isConnected) btn.dataset.copied = 'false'; }, 1500); }
    };
    const fail = () => toast(`Clipboard blocked. Select and copy manually: ${text}`, 'amber', 'warning');
    try {
      if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(ok, fail);
      else fail();
    } catch (_) { fail(); }
  }

  const copyBtn = (text, fk) => h('button', {
    type: 'button', class: 'copybtn', 'aria-label': `Copy ${text}`, title: 'Copy', dataset: { fk, copied: 'false' },
    on: { click: (e) => copyText(text, e.currentTarget) },
  }, iconEl('copy', 15));

  function section(id, icon, title, sub, body, cls) {
    return h('section', { class: `card hsec${cls ? ` ${cls}` : ''}`, 'aria-labelledby': `h-${id}`, dataset: { section: id } },
      h('div', { class: 'hsec__head' },
        h('span', { class: 'hsec__icon' }, iconEl(icon)),
        h('div', null, h('h2', { class: 'hsec__title', id: `h-${id}` }, title), sub ? h('p', { class: 'hsec__sub' }, sub) : null)),
      body);
  }

  /* ------------------------------------------------------------------ sections */

  function cliSection() {
    const secMatch = hit('Core CLI Commands orch');
    const groups = [];
    CLI.forEach((g) => {
      const rows = g.rows.filter(([c, d]) => secMatch && !q ? true : hit(c, d, g.title, g.sub));
      if (!rows.length) return;
      shownCount += rows.length;
      const expanded = q ? true : open.has(g.id);
      groups.push(h('div', { class: 'grp' },
        h('button', {
          type: 'button', class: 'grp__btn', 'aria-expanded': String(expanded), 'aria-controls': `grp-${g.id}`, dataset: { fk: `grp-${g.id}` },
          on: { click: () => { if (open.has(g.id)) open.delete(g.id); else open.add(g.id); render(); } },
        }, h('span', { class: 'grp__icon' }, iconEl(g.icon)),
        h('span', { class: 'grp__txt' }, h('span', { class: 'grp__title' }, g.title), h('span', { class: 'grp__sub' }, g.sub)),
        h('span', { class: 'grp__chev' }, iconEl('chevron'))),
        h('div', { class: 'grp__panel', id: `grp-${g.id}`, hidden: expanded ? null : true },
          rows.map(([c, d]) => h('div', { class: 'cli' },
            h('pre', null, h('code', null, h('span', { class: 'cmd' }, c), h('span', { class: 'cm' }, d))),
            copyBtn(c, `copy-${c}`))))));
    });
    return groups.length ? section('cli', 'terminal', 'Core CLI Commands (orch)', 'Essential commands for orchestration and control.', h('div', { class: 'hsec__body' }, groups)) : null;
  }

  function exitSection() {
    const rows = EXIT.filter(([c, t, d]) => hit('exit code', c, t, d));
    if (!rows.length) return null;
    shownCount += rows.length;
    return section('exit', 'layers', 'Exit Codes (Deterministic)', 'Process exit codes and their meaning.',
      h('table', { class: 'table' },
        h('caption', { class: 'sr-only' }, 'Exit codes'),
        h('thead', null, h('tr', null, h('th', { scope: 'col' }, 'Code'), h('th', { scope: 'col' }, 'Meaning'))),
        h('tbody', null, rows.map(([c, t, d]) => h('tr', null,
          h('td', null, h('span', { class: `codebox codebox--${c}` }, String(c))),
          h('td', { class: 'meaning' }, h('strong', null, t), ` - ${d}`))))));
  }

  function envSection() {
    const rows = ENV.filter(([k, v]) => hit('environment tunables variable', k, v));
    if (!rows.length) return null;
    shownCount += rows.length;
    return section('env', 'gear', 'Environment Tunables', 'Common environment variables for control and policy.',
      h('table', { class: 'table' },
        h('caption', { class: 'sr-only' }, 'Environment variables'),
        h('thead', null, h('tr', null, h('th', { scope: 'col' }, 'Variable'), h('th', { scope: 'col' }, 'Example / Default'), h('th', { scope: 'col' }, h('span', { class: 'sr-only' }, 'Copy')))),
        h('tbody', null, rows.map(([k, v]) => h('tr', null,
          h('td', null, h('code', { class: 'envvar' }, k)),
          h('td', null, h('code', { class: 'envval' }, `= ${v}`)),
          h('td', { class: 'act' }, copyBtn(`${k}=${v}`, `copy-${k}`)))))));
  }

  function keysSection() {
    const rows = KEYS.filter((k) => hit('keyboard shortcuts', k.keys.join(' '), k.title, k.sub));
    if (!rows.length) return null;
    shownCount += rows.length;
    return section('keys', 'terminal', 'Keyboard Shortcuts', 'Fast navigation and common actions in the UI.',
      h('table', { class: 'table' },
        h('caption', { class: 'sr-only' }, 'Keyboard shortcuts'),
        h('thead', null, h('tr', null, h('th', { scope: 'col' }, 'Shortcut'), h('th', { scope: 'col' }, 'Action'))),
        h('tbody', null, rows.map((k) => h('tr', null,
          h('td', null, h('span', { class: 'keys' }, k.keys.flatMap(([t], i) => [i && k.joiner ? h('span', null, k.joiner) : null, h('kbd', null, t)]))),
          h('td', null, h('div', { class: 'act-title' }, k.title), h('div', { class: 'act-sub' }, k.sub)))))));
  }

  function guarSection() {
    const rows = GUARANTEES.filter(([t, d]) => hit('architecture guarantees', t, d));
    if (!rows.length) return null;
    shownCount += rows.length;
    return section('arch', 'layers', 'Architecture Guarantees', null,
      h('ul', { class: 'guar' }, rows.map(([t, d]) => h('li', null, iconEl('checkcircle'), h('div', null, h('strong', null, `${t} `), h('span', { class: 'muted' }, `- ${d}`))))), 'hsec--green');
  }

  /* ------------------------------------------------------------------ render */

  function render() {
    shownCount = 0;
    const cols = [[cliSection()], [exitSection(), envSection()], [keysSection(), guarSection()]];
    UI.preserveFocus(() => {
      cols.forEach((c, i) => $(`#col-${i + 1}`).replaceChildren(...c.filter(Boolean)));
    });
    const none = shownCount === 0;
    $('#hp-none').hidden = !none;
    $('#hp-grid').hidden = none;
    $('#hp-none-q').textContent = q;
    $('#hp-status').textContent = q ? (none ? 'No results' : `${shownCount} result${shownCount === 1 ? '' : 's'}`) : '';
  }

  /* ------------------------------------------------------------------ wiring */

  const input = $('#hp-q');
  $('#hp-icon').replaceChildren(iconEl('help'));
  $('#hp-search-icon').replaceChildren(iconEl('search', 18));
  input.addEventListener('input', () => { q = norm(input.value.trim()); render(); });
  input.addEventListener('keydown', (e) => { if (e.key === 'Escape' && input.value) { e.stopPropagation(); input.value = ''; q = ''; render(); } });
  $('#hp-clear').addEventListener('click', () => { input.value = ''; q = ''; render(); input.focus(); });

  const typing = (t) => t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName));
  let chord = 0;
  const GO = { c: '01-chat-home.html', m: '03-execution-monitor.html', p: '02-workflows-plan.html' };
  document.addEventListener('keydown', (e) => {
    if (e.ctrlKey || e.metaKey || e.altKey || typing(e.target) || document.querySelector('dialog[open]')) return;
    if (e.key === '/') { e.preventDefault(); input.focus(); input.select(); return; }
    if (e.shiftKey && e.key.toLowerCase() === 'a') { e.preventDefault(); window.HerdrShell.openDrawer(); return; }
    if (e.shiftKey) return;
    const k = e.key.toLowerCase();
    if (chord && Date.now() < chord && GO[k]) { chord = 0; e.preventDefault(); window.location.href = GO[k]; return; }
    chord = k === 'g' ? Date.now() + 1500 : 0;
  });

  window.HerdrShell.registerCommands([
    { group: 'Help', label: 'Help: search reference', icon: 'search', hint: '/', run: () => input.focus() },
    { group: 'Help', label: 'Help: copy `orch doctor`', icon: 'copy', hint: 'cli', run: () => copyText('orch doctor') },
    { group: 'Help', label: 'Help: copy `orch status`', icon: 'copy', hint: 'cli', run: () => copyText('orch status') },
    { group: 'Help', label: 'Help: expand all command groups', icon: 'chevron', hint: 'cli', run: () => { CLI.forEach((g) => open.add(g.id)); render(); } },
    { group: 'Help', label: 'Help: collapse all command groups', icon: 'chevron', hint: 'cli', run: () => { open.clear(); render(); } },
  ]);

  render();
}());
