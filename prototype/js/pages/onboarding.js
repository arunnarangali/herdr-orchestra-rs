/* Screen 10 — Onboarding: six-step workspace preflight. Progress persists under herdr.onboarding.state. */
(function () {
  'use strict';

  const Store = window.OrchDemoStore;
  const UI = window.HerdrUI;
  const { h, iconEl, toast } = UI;
  const $ = (sel, root = document) => root.querySelector(sel);
  const LS = 'herdr.onboarding.state';
  const DEFAULT_PATH = '/home/arunn/dev/my-app';

  const downloadIcon = () => {
    const t = document.createElement('template');
    t.innerHTML = '<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4M7 10l5 5 5-5M12 15V3"/></svg>';
    return t.content.firstElementChild;
  };

  const STEPS = [
    { n: 1, icon: 'terminal', title: 'Step 1: Orchestra Binary (orch)', desc: 'Check that the orch CLI is installed and available in your PATH.' },
    { n: 2, icon: 'pulse', title: 'Step 2: Herdr Terminal Runtime', desc: 'Verify the Herdr daemon/runtime is reachable and responding.' },
    { n: 3, icon: 'robot', title: 'Step 3: Installed AI Coding Agents', desc: 'Detect and verify available AI coding agent CLIs and login state.' },
    { n: 4, icon: 'layers', title: 'Step 4: Orchestration Skill Wrappers', desc: 'Ensure agent skill wrappers are installed for orchestration (tool-use, project context, verification, etc.).' },
    { n: 5, icon: 'branch', title: 'Step 5: Git Repository State', desc: 'Check the selected repository for a clean working tree and default branch.' },
    { n: 6, icon: 'folder', title: 'Step 6: Select Project & Start Lead', desc: 'Choose a project folder (existing Git repo) and start the orchestration lead.' },
  ];

  let saved = load();
  let statuses = {};
  let running = false;
  let installing = false;
  let message = '';

  function load() {
    try {
      const s = JSON.parse(localStorage.getItem(LS));
      if (s && typeof s === 'object') return { installed: !!s.installed, skipped: s.skipped || {}, path: s.path || DEFAULT_PATH, done: !!s.done, active: s.active || 0 };
    } catch (_) { /* no saved state */ }
    return { installed: false, skipped: {}, path: DEFAULT_PATH, done: false, active: 0 };
  }
  function persist() {
    try { localStorage.setItem(LS, JSON.stringify(saved)); } catch (_) { /* storage unavailable */ }
  }

  function compute() {
    const st = { 1: 'passed', 2: 'passed', 3: 'passed', 5: 'passed' };
    st[4] = saved.installed ? 'passed' : saved.skipped[4] ? 'skipped' : 'action';
    st[6] = saved.done ? 'passed' : 'pending';
    return st;
  }
  const finished = (n) => statuses[n] === 'passed' || statuses[n] === 'skipped';
  const firstOpen = () => { for (let n = 1; n <= 6; n += 1) if (!finished(n)) return n; return 6; };
  const active = () => saved.active || firstOpen();

  /* ------------------------------------------------------------------ step results */

  const statusHead = (st, label) => {
    const map = {
      passed: ['checkcircle', 'Passed'], action: ['warning', 'Action Needed'], skipped: ['circle', 'Skipped'],
      pending: ['circle', 'Not started'], running: ['spinner', 'Checking...'],
    };
    const [icon, text] = map[st];
    const ic = iconEl(icon);
    if (st === 'running') ic.classList.add('spin');
    return h('div', { class: 'res__status' }, ic, label || text);
  };

  function result(step) {
    const st = statuses[step.n] || 'pending';
    if (st === 'running') return [statusHead('running')];
    if (st === 'pending' && step.n < 6) return [statusHead('pending', 'Waiting to run')];
    switch (step.n) {
      case 1: return [statusHead(st), h('div', { class: 'res__line' }, 'Found v5.0.0-dev at', h('span', { class: 'path-chip' }, '/usr/local/bin/orch'))];
      case 2: return [statusHead(st), h('div', { class: 'res__line' }, 'Herdr daemon reachable via', h('span', { class: 'path-chip' }, 'HERDR_BIN'), '· Socket active')];
      case 3: return [statusHead(st, 'Passed (3 Found)'), h('div', { class: 'agentchips' },
        h('span', { class: 'achip' }, iconEl('bolt'), h('span', null, 'claude'), '(logged in)'),
        h('span', { class: 'achip' }, iconEl('cube'), h('span', null, 'codex'), '(logged in)'),
        h('span', { class: 'achip achip--cyan' }, iconEl('code'), h('span', null, 'opencode'), '(ready)'))];
      case 4:
        if (st === 'passed') return [statusHead('passed'), h('div', { class: 'res__line' }, 'Wrappers installed for claude, codex, opencode in', h('span', { class: 'path-chip' }, '~/.codex/skills'))];
        return [
          statusHead(st),
          h('div', { class: 'res__line' }, st === 'skipped' ? 'Skills missing for codex (skipped by you)' : 'Skills missing for codex', h('span', { class: 'path-chip' }, '(~/.codex/skills)')),
          h('div', { class: 'res__act' },
            h('button', { type: 'button', class: 'btn', disabled: installing ? true : null, dataset: { fk: 'install' }, on: { click: install } },
              downloadIcon(), installing ? 'Running orch agents sync...' : 'Install Skill Wrappers (orch agents sync)'),
            h('span', { class: 'res__note' }, iconEl('help'), 'Recorded in uninstall manifest.')),
        ];
      case 5: return [statusHead(st), h('div', { class: 'res__line' }, 'Clean Git repo detected · Default branch:', h('span', { class: 'path-chip' }, 'main'))];
      default: return step6();
    }
  }

  function step6() {
    if (saved.done) {
      return [statusHead('passed', 'Lead started'), h('div', { class: 'res__line' }, 'Project opened at', h('span', { class: 'path-chip' }, saved.path))];
    }
    const input = h('input', {
      type: 'text', id: 'proj-path', value: saved.path, spellcheck: 'false', autocomplete: 'off', dataset: { fk: 'path' },
      'aria-label': 'Project folder path', 'aria-describedby': 'path-error', required: true,
      on: {
        input: (e) => { saved.path = e.target.value; persist(); showPathError(''); },
        keydown: (e) => { if (e.key === 'Enter') { e.preventDefault(); startLead(); } },
      },
    });
    const box = h('div', { class: 'pathrow__box', id: 'path-box' }, iconEl('folder'), input);
    return [
      h('div', { class: 'pathrow' }, box,
        h('button', { type: 'button', class: 'btn', 'aria-label': 'Browse for a folder', title: 'Browse', dataset: { fk: 'browse' }, on: { click: openBrowse } }, iconEl('folder'))),
      h('p', { class: 'field-error', id: 'path-error', role: 'alert' }),
      h('button', { type: 'button', class: 'btn btn--emerald btn--solid btn--block btn--lg', dataset: { fk: 'start' }, on: { click: startLead } },
        iconEl('play'), 'Open Project & Start Lead'),
    ];
  }

  function showPathError(text) {
    const e = $('#path-error');
    if (e) e.textContent = text;
    const b = $('#path-box');
    if (b) b.dataset.invalid = text ? 'true' : 'false';
    const i = $('#proj-path');
    if (i) i.setAttribute('aria-invalid', text ? 'true' : 'false');
  }

  function validatePath(p) {
    const v = p.trim();
    if (!v) return 'Enter the path of a Git repository.';
    if (!/^(\/|~\/)/.test(v)) return 'Use an absolute path such as /home/you/dev/my-app.';
    if (v.split('/').includes('..')) return 'Path must not contain ".." segments.';
    if (/[\0\n]/.test(v) || v.length > 255) return 'Path contains invalid characters.';
    if (v === '/' || v === '~/') return 'Choose a project folder, not the filesystem root.';
    return '';
  }

  /* ------------------------------------------------------------------ rendering */

  function renderSteps() {
    const cur = active();
    $('#steps').replaceChildren(...STEPS.map((s) => {
      const st = statuses[s.n] || 'pending';
      const label = { passed: 'passed', action: 'action needed', skipped: 'skipped', pending: 'not started', running: 'checking' }[st];
      return h('li', { class: `step${s.n === cur ? ' is-active' : ''}`, dataset: { step: s.n, status: st }, 'aria-current': s.n === cur ? 'step' : null },
        h('div', { class: 'step__rail' }, h('button', {
          type: 'button', class: 'step__num', 'aria-label': `Step ${s.n}: ${label}`, dataset: { fk: `num-${s.n}` },
          on: { click: () => goTo(s.n) },
        }, String(s.n))),
        h('section', { class: `card scard${s.n === 6 ? ' scard--start' : ''}`, 'aria-label': s.title },
          h('span', { class: 'scard__icon' }, iconEl(s.icon)),
          h('div', { class: 'scard__main' }, h('h2', { class: 'scard__title' }, s.title), h('p', { class: 'scard__desc' }, s.desc)),
          h('div', { class: 'scard__res' }, result(s))));
    }));
  }

  function renderBar() {
    const cur = active();
    const done = [1, 2, 3, 4, 5, 6].filter(finished).length;
    $('#ob-count').textContent = `Step ${cur} of 6 · ${done} complete`;
    const bar = $('#ob-bar');
    bar.setAttribute('aria-valuenow', String(done));
    bar.firstElementChild.style.width = `${(done / 6) * 100}%`;
    $('#back-btn').disabled = cur <= 1;
    $('#next-btn').disabled = cur >= 6;
    const skip = $('#skip-btn');
    skip.disabled = cur >= 6 || finished(cur);
    $('#next-btn').replaceChildren('Next', iconEl('chevron'));
    $('#next-btn').lastElementChild.style.rotate = '-90deg';
    const ob = $('#ob-msg');
    ob.textContent = message;
    $('#rerun-btn').disabled = running;
    $('#rerun-btn').replaceChildren(iconEl('sync'), running ? 'Checking...' : 'Re-run All Checks');
    const complete = saved.done;
    const doneBox = $('#ob-done');
    doneBox.hidden = !complete;
    if (complete) $('#done-sub').textContent = `Lead started for ${saved.path}. Preflight finished with ${done} of 6 steps complete.`;
  }

  function render() {
    UI.preserveFocus(() => { renderSteps(); renderBar(); });
  }

  /* ------------------------------------------------------------------ actions */

  function goTo(n) { saved.active = Math.max(1, Math.min(6, n)); message = ''; persist(); render(); }

  function next() {
    const cur = active();
    if (!finished(cur) && cur !== 6) {
      message = `Step ${cur} needs attention. Fix it or skip it before continuing.`;
      render();
      const t = document.querySelector(`.step[data-step="${cur}"] button[data-fk="install"]`);
      if (t) t.focus();
      return;
    }
    goTo(cur + 1);
  }

  function skip() {
    const cur = active();
    if (cur >= 6 || finished(cur)) return;
    saved.skipped[cur] = true;
    statuses = compute();
    toast(`Step ${cur} skipped`, 'amber', 'warning');
    goTo(cur + 1);
  }

  function install() {
    if (installing) return;
    installing = true;
    message = '';
    render();
    setTimeout(() => {
      installing = false;
      saved.installed = true;
      delete saved.skipped[4];
      statuses = compute();
      if (active() === 4) saved.active = 5;
      persist();
      toast('Skill wrappers installed for codex', 'emerald', 'check');
      render();
    }, 900);
  }

  function rerun() {
    if (running) return;
    running = true;
    message = '';
    const final = compute();
    let i = 0;
    statuses = {};
    const tick = () => {
      if (i > 0) statuses[i] = final[i];
      i += 1;
      if (i <= 5) { statuses[i] = 'running'; render(); setTimeout(tick, 320); return; }
      statuses = compute();
      running = false;
      toast(statuses[4] === 'action' ? 'Checks finished: 1 step needs action' : 'All checks passed', statuses[4] === 'action' ? 'amber' : 'emerald', statuses[4] === 'action' ? 'warning' : 'check');
      render();
    };
    tick();
  }

  function startLead() {
    const err = validatePath(saved.path);
    if (err) { showPathError(err); const i = $('#proj-path'); if (i) i.focus(); return; }
    const blocked = [1, 2, 3, 4, 5].find((n) => !finished(n));
    if (blocked) {
      message = `Complete step ${blocked} (or skip it) before starting the lead.`;
      saved.active = blocked;
      persist();
      render();
      return;
    }
    saved.path = saved.path.trim();
    saved.done = true;
    saved.active = 6;
    persist();
    statuses = compute();
    const known = Store.projects().find((p) => p.path === saved.path);
    if (known) Store.actions.switchProject(known.id);
    message = '';
    toast('Project opened, lead is starting', 'emerald', 'play');
    render();
    $('#ob-done').focus();
  }

  function reset() {
    try { localStorage.removeItem(LS); } catch (_) { /* ignore */ }
    saved = load();
    message = '';
    installing = false;
    statuses = compute();
    toast('Onboarding progress cleared', 'cyan', 'sync');
    render();
    $('#reset-btn').focus();
  }

  /* ------------------------------------------------------------------ browse dialog */

  const dlg = $('#browse-dialog');
  UI.enhanceDialog(dlg);
  $('#browse-x').replaceChildren(iconEl('x'));
  $('#browse-x').addEventListener('click', () => dlg.close());
  $('#browse-close').addEventListener('click', () => dlg.close());
  dlg.addEventListener('close', () => { const b = document.querySelector('[data-fk="browse"]'); if (b) b.focus(); });
  function openBrowse() {
    const paths = [...new Set([DEFAULT_PATH, ...Store.projects().map((p) => p.path), '/home/arunn/dev/orch-sandbox'])];
    $('#browse-list').replaceChildren(...paths.map((p) => h('li', null, h('button', {
      type: 'button', on: { click: () => { saved.path = p; persist(); dlg.close(); render(); toast('Folder selected', 'cyan', 'folder'); } },
    }, iconEl('folder'), p, Store.projects().some((x) => x.path === p) ? h('span', { class: 'muted' }, 'registered project') : null))));
    if (!dlg.open) dlg.showModal();
  }

  /* ------------------------------------------------------------------ boot */

  $('#ob-icon').replaceChildren(iconEl('rocket'));
  $('#done-icon').replaceChildren(iconEl('checkcircle'));
  $('#done-link').replaceChildren(iconEl('chat'), 'Go to Chat / Home');
  $('#reset-btn').replaceChildren(iconEl('trash'), 'Reset progress');
  $('#back-btn').replaceChildren(iconEl('chevron'), 'Back');
  $('#back-btn').firstElementChild.style.rotate = '90deg';
  $('#skip-btn').textContent = 'Skip step';
  $('#back-btn').addEventListener('click', () => goTo(active() - 1));
  $('#next-btn').addEventListener('click', next);
  $('#skip-btn').addEventListener('click', skip);
  $('#rerun-btn').addEventListener('click', rerun);
  $('#reset-btn').addEventListener('click', reset);

  window.HerdrShell.registerCommands([
    { group: 'Onboarding', label: 'Onboarding: re-run all checks', icon: 'sync', hint: 'preflight', run: rerun },
    { group: 'Onboarding', label: 'Onboarding: next step', icon: 'chevron', hint: 'next', run: next },
    { group: 'Onboarding', label: 'Onboarding: install skill wrappers', icon: 'layers', hint: 'orch agents sync', run: () => { if (statuses[4] !== 'passed') install(); else toast('Skill wrappers already installed', 'cyan', 'check'); } },
    { group: 'Onboarding', label: 'Onboarding: reset progress', icon: 'trash', hint: 'reset', run: reset },
  ]);

  statuses = compute();
  render();
}());
