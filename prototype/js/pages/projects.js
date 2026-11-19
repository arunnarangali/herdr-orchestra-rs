/* Screen 05 — Projects & Workspaces. Project cards, add/create/close flows, execution history. */
(function () {
  'use strict';

  const Store = window.OrchDemoStore;
  const UI = window.HerdrUI;
  const { h, iconEl, toast } = UI;
  const $ = (sel, root = document) => root.querySelector(sel);
  const KEY = 'herdr.projects.state';

  /* ------------------------------------------------------------------ seed + persisted state */

  const SEED = {
    'herdr-orchestra': { port: 49152, clean: true, history: [
      { id: '#8417', plan: 'Auth Flow & Billing', branch: 'feature/auth-billing', merged: 5, total: 6, duration: '2h 03m', status: 'stopped' },
      { id: '#8403', plan: 'User Management', branch: 'feature/user-mgmt', merged: 8, total: 8, duration: '1h 48m', status: 'completed' },
      { id: '#8382', plan: 'Payment Webhooks', branch: 'feature/payments', merged: 4, total: 8, duration: '3h 22m', status: 'failed' },
      { id: '#8367', plan: 'Analytics Dashboard', branch: 'feature/analytics', merged: 8, total: 8, duration: '1h 36m', status: 'archived' },
      { id: '#8351', plan: 'Search Indexing', branch: 'feature/search', merged: 6, total: 6, duration: '58m', status: 'completed' },
      { id: '#8339', plan: 'Rate Limiting', branch: 'feature/rate-limit', merged: 5, total: 5, duration: '1h 12m', status: 'completed' },
    ] },
    streamforge: { port: 49155, clean: true, history: [
      { id: '#8362', plan: 'Kafka Source Connector', branch: 'feature/kafka-source', merged: 6, total: 6, duration: '2h 11m', status: 'completed' },
      { id: '#8348', plan: 'Checkpoint Compaction', branch: 'feature/compaction', merged: 4, total: 7, duration: '2h 54m', status: 'failed' },
      { id: '#8331', plan: 'Sink Retries', branch: 'feature/sink-retries', merged: 5, total: 5, duration: '1h 03m', status: 'completed' },
      { id: '#8317', plan: 'Schema Registry Client', branch: 'feature/schema-registry', merged: 7, total: 7, duration: '1h 44m', status: 'archived' },
      { id: '#8300', plan: 'Windowed Aggregations', branch: 'feature/windows', merged: 3, total: 6, duration: '1h 20m', status: 'stopped' },
    ] },
  };
  const STATUS = {
    completed: ['emerald', 'Completed', 'checkcircle'], stopped: ['amber', 'Stopped', 'stop'], failed: ['coral', 'Failed', 'warning'],
    archived: ['neutral', 'Archived', 'package'], running: ['cyan', 'Running', 'spinner'], paused: ['amber', 'Paused', 'pause'],
  };

  const state = (() => {
    const base = { server: {}, closed: [], added: [], statusOv: {} };
    try { return { ...base, ...JSON.parse(localStorage.getItem(KEY) || '{}') }; } catch (_) { return base; }
  })();
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(state)); } catch (_) { /* storage unavailable */ } };

  let histProject = null;
  let showAll = false;
  let menu = null;
  let focusMenu = false;
  let closing = null;

  const badgeCls = (tone) => (tone === 'neutral' ? 'badge' : `badge badge--${tone}`);

  /* ------------------------------------------------------------------ project model */

  function projectList() {
    const store = Store.projects().map((p) => {
      const d = Store.data(p.id);
      const run = Store.run(p.id);
      const seed = SEED[p.id] || { port: 49160, clean: true, history: [] };
      return {
        id: p.id, name: p.name, path: p.path, branch: p.branch, base: Store.workflow(p.id).base, attention: p.attention,
        port: seed.port, clean: seed.clean, worktrees: d.agents.length, linked: true,
        run: { plan: run.plan, percent: run.percent, state: run.state, id: run.id },
      };
    });
    const added = state.added.map((a) => ({ ...a, attention: 0, worktrees: 0, linked: false, run: null }));
    return [...store, ...added].filter((p) => !state.closed.includes(p.id));
  }

  const serverOn = (p) => state.server[p.id] !== false;

  function historyFor(id) {
    const store = Store.projects().find((p) => p.id === id);
    const rows = [];
    if (store) {
      const r = Store.run(id);
      const wf = Store.workflow(id);
      const mergedN = wf.milestones.filter((m) => m.state === 'merged').length;
      rows.push({ id: `#${r.id}`, plan: r.plan, branch: r.branch, merged: mergedN, total: wf.milestones.length, duration: fmtDur(r.elapsed_s), status: r.state === 'live' ? 'running' : r.state, live: true });
    }
    const seed = SEED[id] ? SEED[id].history : [];
    return rows.concat(seed.map((x) => ({ ...x, status: state.statusOv[x.id] || x.status })));
  }

  function fmtDur(secs) {
    const hh = Math.floor(secs / 3600);
    const mm = Math.floor((secs % 3600) / 60);
    return hh ? `${hh}h ${String(mm).padStart(2, '0')}m` : `${mm}m`;
  }

  async function copyText(text) {
    try { await navigator.clipboard.writeText(text); toast(`Copied ${text}`, 'cyan', 'copy'); }
    catch (_) { toast(`Copy is blocked in this context. Value: ${text}`, 'amber', 'warning'); }
  }

  /* ------------------------------------------------------------------ kebab menu */

  function dotsIcon() {
    const t = document.createElement('template');
    t.innerHTML = '<svg class="icon" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true" focusable="false"><circle cx="12" cy="5" r="1.7"/><circle cx="12" cy="12" r="1.7"/><circle cx="12" cy="19" r="1.7"/></svg>';
    return t.content.firstElementChild;
  }


  function kebab(key, label, items) {
    const open = menu === key;
    const btn = h('button', {
      type: 'button', class: 'btn btn--ghost btn--icon btn--sm kebab__btn', 'data-fk': `kb-${key}`, 'aria-label': label,
      'aria-haspopup': 'menu', 'aria-expanded': String(open),
      on: { click: (e) => { e.stopPropagation(); menu = open ? null : key; focusMenu = !open; render(); } },
    }, dotsIcon());
    const list = open ? h('ul', { class: 'kebab__menu card', role: 'menu', 'aria-label': label },
      items.map((it, i) => h('li', { role: 'none' }, h('button', {
        type: 'button', role: 'menuitem', class: `kebab__item${it.tone ? ` text-${it.tone}` : ''}`, 'data-fk': `mi-${key}-${i}`,
        on: { click: (e) => { e.stopPropagation(); menu = null; render(); it.run(); } },
      }, iconEl(it.icon, 14), it.label)))) : null;
    return h('span', { class: 'kebab' }, btn, list);
  }

  document.addEventListener('click', (e) => { if (menu && !e.target.closest('.kebab')) { menu = null; render(); } });
  document.addEventListener('keydown', (e) => {
    if (!menu) return;
    const items = [...document.querySelectorAll('.kebab__menu .kebab__item')];
    const i = items.indexOf(document.activeElement);
    if (e.key === 'Escape') {
      const key = menu; menu = null; render();
      const b = document.querySelector(`[data-fk="kb-${key}"]`); if (b) b.focus();
    } else if (e.key === 'ArrowDown' && items.length) { e.preventDefault(); items[(i + 1) % items.length].focus(); }
    else if (e.key === 'ArrowUp' && items.length) { e.preventDefault(); items[(i - 1 + items.length) % items.length].focus(); }
    else if (e.key === 'Tab') { menu = null; render(); }
  });

  /* ------------------------------------------------------------------ project card */

  function openRun(p) {
    return () => { if (p.linked) Store.actions.switchProject(p.id); };
  }

  function projectCard(p) {
    const active = p.id === Store.project;
    const on = serverOn(p);
    const done = p.run && p.run.percent >= 100;
    const attnBadge = p.attention
      ? h('span', { class: 'badge badge--amber' }, `Attention: ${p.attention}`)
      : null;
    const headBadge = active ? h('span', { class: 'badge badge--emerald badge--pill' }, h('span', { class: 'dot dot--emerald', 'aria-hidden': 'true' }), 'Active')
      : p.attention ? attnBadge : h('span', { class: 'badge badge--pill' }, 'Idle');
    const closeBtn = h('button', { type: 'button', class: 'btn', 'data-fk': `close-${p.id}`, 'aria-haspopup': 'dialog', on: { click: () => askClose(p) } }, iconEl('x', 14), 'Close Project');

    let actions;
    if (!p.run) {
      actions = [h('a', { class: 'btn btn--emerald btn--solid', href: '01-chat-home.html' }, iconEl('chat', 14), 'Plan a Feature'), closeBtn];
    } else if (done) {
      actions = [
        h('a', { class: 'btn btn--cyan', href: '07-reports.html', on: { click: openRun(p) } }, iconEl('doc', 14), 'View Summary'),
        h('button', { type: 'button', class: 'btn btn--emerald', 'data-fk': `pr-${p.id}`, on: { click: () => toast(`Opening a pull request for ${p.branch} into ${p.base} (demo)`, 'emerald', 'branch') } }, iconEl('branch', 14), 'Finish & Open PR'),
        closeBtn];
    } else {
      actions = [
        h('a', { class: 'btn btn--emerald btn--solid', href: '03-execution-monitor.html', on: { click: openRun(p) } }, iconEl('external', 14), 'Open Run'),
        h('button', {
          type: 'button', class: `btn ${on ? 'btn--coral' : 'btn--emerald'}`, 'data-fk': `srv-${p.id}`,
          on: { click: () => { state.server[p.id] = !on; save(); toast(on ? `Server for ${p.name} stopped` : `Server for ${p.name} listening on 127.0.0.1:${p.port}`, on ? 'coral' : 'emerald', on ? 'stop' : 'play'); render(); } },
        }, iconEl(on ? 'stop' : 'play', 14), on ? 'Stop Server' : 'Start Server'),
        closeBtn];
    }

    const pct = p.run ? p.run.percent : 0;
    const runState = p.run && p.run.state !== 'live' ? p.run.state : null;
    return h('article', { class: `card pcard${active ? ' card--tone tone-emerald' : ''}`, 'aria-labelledby': `pc-${p.id}` },
      h('header', { class: 'pcard__head' },
        h('span', { class: `pcard__logo${p.linked && p.id === 'streamforge' ? ' pcard__logo--blue' : ''}` }, iconEl('cube', 28)),
        h('div', { class: 'pcard__id' },
          h('h2', { class: 'pcard__name', id: `pc-${p.id}` }, p.name),
          h('div', { class: 'pcard__pathlabel muted' }, 'Canonical Path',
            h('button', { type: 'button', class: 'iconbtn', 'data-fk': `cp-${p.id}`, 'aria-label': `Copy path of ${p.name}`, on: { click: () => copyText(p.path) } }, iconEl('copy', 14))),
          h('div', { class: 'mono pcard__path' }, p.path)),
        headBadge,
        kebab(`p-${p.id}`, `${p.name} actions`, [
          { label: 'Copy path', icon: 'copy', run: () => copyText(p.path) },
          { label: 'Open in Herdr', icon: 'external', run: () => toast(`Opening ${p.name} workspace in Herdr (demo)`, 'cyan', 'external') },
          ...(p.linked && !active ? [{ label: 'Switch to this project', icon: 'folder', run: () => Store.actions.switchProject(p.id) }] : []),
          { label: 'Close project', icon: 'x', tone: 'coral', run: () => askClose(p) },
        ])),
      h('div', { class: 'pcard__cols' },
        h('div', { class: 'pcard__server' },
          h('div', { class: 'pcard__k' }, h('span', { class: `dot dot--${on ? 'emerald' : 'idle'}`, 'aria-hidden': 'true' }), 'Server Status', on ? null : h('span', { class: 'badge' }, 'Stopped')),
          h('div', { class: 'pcard__cmd' },
            h('span', { class: 'mono' }, on ? `orch serve · 127.0.0.1:${p.port}` : 'orch serve · not running'),
            on ? h('span', { class: 'badge badge--emerald' }, 'Token Active') : null,
            on && p.attention ? h('span', { class: 'badge badge--amber' }, `Attention: ${p.attention}`) : null)),
        h('div', { class: 'pcard__git' },
          h('div', { class: 'pcard__gk' }, iconEl('branch', 14), 'Git'),
          h('div', { class: 'pcard__gr' }, iconEl('branch', 14), 'Branch:', h('span', { class: 'mono text-cyan' }, p.branch)),
          h('div', { class: 'pcard__gr' }, iconEl('target', 14), 'Base:', h('span', { class: 'badge badge--mono' }, p.base)),
          h('div', { class: 'pcard__gr pcard__gr--small' },
            h('span', { class: 'text-emerald pcard__clean' }, iconEl('check', 14), p.clean ? 'Clean working tree' : 'Uncommitted changes'),
            p.worktrees ? h('span', { class: 'pcard__wt' }, iconEl('layers', 14), `${p.worktrees} active worktree${p.worktrees === 1 ? '' : 's'}`, h('span', { class: 'mono muted pcard__wtpath' }, `../${p.name}-wt/`)) : null))),
      h('div', { class: 'pcard__run' },
        h('span', { class: 'pcard__runicon' }, iconEl('play', 18)),
        h('div', { class: 'pcard__runbody' },
          h('div', { class: 'pcard__runlabel' }, h('span', { class: 'muted' }, 'Active Run'), p.run ? null : h('span', { class: 'muted' }, 'No active run')),
          p.run ? h('div', { class: 'pcard__runtitle' }, p.run.plan, runState ? h('span', { class: `${badgeCls(runState === 'paused' ? 'amber' : 'coral')} badge--sm` }, runState) : null) : null,
          h('div', { class: `bar${runState ? ` bar--${runState}` : ''}`, role: 'progressbar', 'aria-label': `${p.name} run progress`, 'aria-valuemin': '0', 'aria-valuemax': '100', 'aria-valuenow': String(pct) }, h('i', { style: `width:${pct}%` }))),
        done ? h('span', { class: 'badge badge--emerald badge--mono' }, h('span', { class: 'dot dot--emerald', 'aria-hidden': 'true' }), 'ALL_DONE') : null,
        h('strong', { class: 'pcard__pct' }, `${pct}%`)),
      h('div', { class: 'pcard__actions' }, actions));
  }

  /* ------------------------------------------------------------------ history */

  function historyCard(list) {
    const ids = list.map((p) => p.id);
    if (!ids.includes(histProject)) histProject = ids.includes(Store.project) ? Store.project : ids[0];
    const proj = list.find((p) => p.id === histProject);
    const rows = proj ? historyFor(proj.id) : [];
    const shown = showAll ? rows : rows.slice(0, 5);

    const select = h('select', {
      class: 'select hist__select', id: 'hist-project', 'aria-label': 'History project', 'data-fk': 'hist-project',
      on: { change: (e) => { histProject = e.target.value; showAll = false; render(); } },
    }, list.map((p) => h('option', { value: p.id, selected: p.id === histProject || null }, p.name)));

    const body = shown.map((r) => {
      const [tone, label, icon] = STATUS[r.status] || STATUS.completed;
      const ic = iconEl(icon, 12);
      if (icon === 'spinner') ic.classList.add('spin');
      const partial = r.merged < r.total;
      const resumable = r.status === 'stopped' || r.status === 'paused';
      const act = r.live && !resumable
        ? h('a', { class: 'btn btn--sm btn--cyan hist__act', href: '03-execution-monitor.html', on: { click: openRun(proj) } }, iconEl('external', 14), 'Open Run')
        : resumable
          ? h('button', { type: 'button', class: 'btn btn--sm btn--cyan hist__act', 'data-fk': `resume-${r.id}`, on: { click: () => resume(r) } }, iconEl('play', 14), 'Resume Run')
          : h('a', { class: 'btn btn--sm btn--cyan hist__act', href: '07-reports.html' }, iconEl('doc', 14), 'View Report');
      const items = [
        { label: 'Copy run ID', icon: 'copy', run: () => copyText(r.id) },
        { label: 'Copy branch', icon: 'branch', run: () => copyText(r.branch) },
        ...(!r.live ? [r.status === 'archived'
          ? { label: 'Unarchive run', icon: 'package', run: () => setStatus(r, 'completed') }
          : { label: 'Archive run', icon: 'package', run: () => setStatus(r, 'archived') }] : []),
      ];
      return h('tr', null,
        h('td', { class: 'mono' }, r.id),
        h('td', null, r.plan),
        h('td', null, h('span', { class: 'mono text-cyan' }, r.branch)),
        h('td', { class: partial ? 'text-amber hist__ms' : 'text-emerald hist__ms' }, `${r.merged}/${r.total} Merged`),
        h('td', { class: 'hist__dur' }, r.duration),
        h('td', null, h('span', { class: badgeCls(tone) }, ic, label)),
        h('td', { class: 'hist__actions' }, act, kebab(`r-${proj.id}-${r.id}`, `Actions for run ${r.id}`, items)));
    });

    return h('section', { class: 'card hist', 'aria-labelledby': 'hist-title' },
      h('header', { class: 'card__head hist__head' },
        h('span', { class: 'hist__icon' }, iconEl('clock', 24)),
        h('div', { class: 'hist__titles' },
          h('h2', { class: 'card__title', id: 'hist-title' }, 'Execution History & Previous Runs'),
          h('p', { class: 'muted hist__sub' }, `Recent runs for ${proj ? proj.name : '—'}`)),
        h('label', { class: 'sr-only', for: 'hist-project' }, 'Project'),
        h('div', { class: 'hist__select-wrap' }, iconEl('cube', 14), select),
        h('button', { type: 'button', class: 'btn btn--sm', 'aria-expanded': String(showAll), 'data-fk': 'hist-all', disabled: rows.length <= 5 || null, on: { click: () => { showAll = !showAll; render(); } } }, showAll ? 'Show Less' : 'View All')),
      h('div', { class: 'hist__scroll' },
        h('table', { class: 'table hist__table' },
          h('thead', null, h('tr', null, ['Run ID', 'Plan Title', 'Feature Branch', 'Milestones', 'Duration', 'Status', 'Actions'].map((c) => h('th', { scope: 'col' }, c)))),
          h('tbody', null, shown.length ? body : h('tr', null, h('td', { colspan: '7', class: 'hist__empty' }, 'No runs recorded for this project yet.'))))));
  }

  function setStatus(r, status) {
    state.statusOv[r.id] = status; save();
    toast(`Run ${r.id} ${status === 'archived' ? 'archived' : 'restored'}`, 'cyan', 'package');
    render();
  }

  function resume(r) {
    if (r.live) {
      Store.actions.setRunState('live');
      toast(`Run ${r.id} resumed`, 'emerald', 'play');
    } else {
      state.statusOv[r.id] = 'running'; save();
      toast(`Run ${r.id} resumed from its last merged milestone`, 'emerald', 'play');
      render();
    }
  }

  /* ------------------------------------------------------------------ dialogs */

  const closeDlg = h('dialog', { class: 'modal', id: 'close-dialog', closedby: 'any', 'aria-labelledby': 'close-title' });
  const addDlg = h('dialog', { class: 'modal', id: 'add-dialog', closedby: 'any', 'aria-labelledby': 'add-title' });
  const newDlg = h('dialog', { class: 'modal', id: 'new-dialog', closedby: 'any', 'aria-labelledby': 'new-title' });
  document.body.append(closeDlg, addDlg, newDlg);
  [closeDlg, addDlg, newDlg].forEach(UI.enhanceDialog);

  function askClose(p) {
    if (projectList().length <= 1) { toast('Keep at least one project open', 'amber', 'warning'); return; }
    closing = p;
    closeDlg.replaceChildren(
      h('div', { class: 'modal__head' }, h('h2', { class: 'modal__title', id: 'close-title' }, `Close ${p.name}?`)),
      h('form', { method: 'dialog' },
        h('div', { class: 'modal__body' },
          h('p', null, 'The project leaves this list and its ', h('span', { class: 'mono' }, 'orch serve'), ' process is stopped. Worktrees and run history stay on disk.'),
          p.attention ? h('p', { class: 'text-amber' }, `${p.attention} attention item${p.attention === 1 ? ' is' : 's are'} still open for this project.`) : null),
        h('div', { class: 'modal__foot' },
          h('button', { type: 'submit', class: 'btn', value: 'cancel', autofocus: true }, 'Keep open'),
          h('button', { type: 'submit', class: 'btn btn--coral btn--solid', value: 'close' }, 'Close project'))));
    closeDlg.showModal();
  }
  closeDlg.addEventListener('close', () => {
    const p = closing; closing = null;
    if (p && closeDlg.returnValue === 'close') {
      if (p.linked) state.closed.push(p.id);
      else state.added = state.added.filter((a) => a.id !== p.id);
      save();
      if (p.linked && p.id === Store.project) {
        const other = projectList().find((x) => x.linked && x.id !== p.id);
        if (other) Store.actions.switchProject(other.id);
      }
      toast(`${p.name} closed`, 'coral', 'x');
      render();
    }
    closeDlg.returnValue = '';
  });

  function nameFromPath(path) { return path.replace(/\/+$/, '').split('/').pop(); }
  function normPath(path) { return path.trim().replace(/\/{2,}/g, '/').replace(/(.)\/+$/, '$1'); }

  function addProject(name, path, branch) {
    const id = name.toLowerCase();
    state.added.push({ id, name, path, branch, base: 'main', port: 49160 + state.added.length + 1, clean: true });
    state.closed = state.closed.filter((x) => x !== id);
    save();
    render();
    toast(`${name} added`, 'emerald', 'check');
  }

  function exists(name, path) {
    return projectList().some((p) => p.name.toLowerCase() === name.toLowerCase() || p.path === path);
  }

  function openAdd() {
    const err = h('p', { class: 'form-err text-coral', id: 'add-err', role: 'alert' });
    const path = h('input', { class: 'input mono', id: 'add-path', name: 'path', placeholder: '/home/arun/dev/my-service', autocomplete: 'off', required: true, 'aria-describedby': 'add-err add-hint' });
    const name = h('input', { class: 'input', id: 'add-name', name: 'name', placeholder: 'my-service', autocomplete: 'off', required: true });
    path.addEventListener('input', () => { if (!name.dataset.touched) name.value = nameFromPath(path.value); });
    name.addEventListener('input', () => { name.dataset.touched = '1'; });
    const form = h('form', { method: 'dialog', novalidate: true, on: { submit: (e) => {
      e.preventDefault();
      const p = normPath(path.value);
      const n = name.value.trim();
      if (!p.startsWith('/') || p.length < 2) { err.textContent = 'Enter an absolute folder path that starts with /.'; path.focus(); return; }
      if (!/^[\w.-]+$/.test(n)) { err.textContent = 'Use letters, numbers, dots, dashes or underscores for the project name.'; name.focus(); return; }
      if (exists(n, p)) { err.textContent = 'That project or folder is already in the list.'; path.focus(); return; }
      addDlg.close('added');
      addProject(n, p, 'main');
    } } },
      h('div', { class: 'modal__body' },
        h('div', { class: 'field' }, h('label', { for: 'add-path' }, 'Folder path'), path, h('span', { class: 'hint', id: 'add-hint' }, 'The repository root. It is resolved to one canonical path.')),
        h('div', { class: 'field' }, h('label', { for: 'add-name' }, 'Project name'), name),
        err),
      h('div', { class: 'modal__foot' },
        h('button', { type: 'button', class: 'btn', on: { click: () => addDlg.close('cancel') } }, 'Cancel'),
        h('button', { type: 'submit', class: 'btn btn--emerald btn--solid' }, iconEl('plus', 14), 'Add folder')));
    addDlg.replaceChildren(h('div', { class: 'modal__head' }, h('h2', { class: 'modal__title', id: 'add-title' }, 'Add Existing Folder')), form);
    addDlg.showModal();
    path.focus();
  }

  function openNew() {
    const err = h('p', { class: 'form-err text-coral', id: 'new-err', role: 'alert' });
    const name = h('input', { class: 'input', id: 'new-name', name: 'name', placeholder: 'billing-service', autocomplete: 'off', required: true });
    const parent = h('input', { class: 'input mono', id: 'new-parent', name: 'parent', value: '/mnt/workspace/projects', autocomplete: 'off' });
    const tpl = h('select', { class: 'select', id: 'new-tpl', name: 'tpl' },
      h('option', { value: 'empty' }, 'Empty repository'), h('option', { value: 'rust' }, 'Rust workspace'), h('option', { value: 'web' }, 'TypeScript web app'));
    const git = h('input', { type: 'checkbox', id: 'new-git', checked: true });
    const form = h('form', { method: 'dialog', novalidate: true, on: { submit: (e) => {
      e.preventDefault();
      const n = name.value.trim();
      const base = normPath(parent.value);
      if (!/^[A-Za-z0-9][\w.-]*$/.test(n)) { err.textContent = 'Start with a letter or number; use letters, numbers, dots, dashes or underscores.'; name.focus(); return; }
      if (!base.startsWith('/')) { err.textContent = 'The parent folder must be an absolute path.'; parent.focus(); return; }
      const full = `${base}/${n}`;
      if (exists(n, full)) { err.textContent = 'A project with that name or path already exists.'; name.focus(); return; }
      newDlg.close('created');
      addProject(n, full, git.checked ? 'main' : 'no git');
    } } },
      h('div', { class: 'modal__body' },
        h('div', { class: 'field' }, h('label', { for: 'new-name' }, 'Project name'), name),
        h('div', { class: 'field' }, h('label', { for: 'new-parent' }, 'Parent folder'), parent),
        h('div', { class: 'field' }, h('label', { for: 'new-tpl' }, 'Starter template'), tpl),
        h('label', { class: 'check', for: 'new-git' }, git, h('span', null, 'Initialize a git repository with a ', h('span', { class: 'mono' }, 'main'), ' branch')),
        err),
      h('div', { class: 'modal__foot' },
        h('button', { type: 'button', class: 'btn', on: { click: () => newDlg.close('cancel') } }, 'Cancel'),
        h('button', { type: 'submit', class: 'btn btn--emerald btn--solid' }, iconEl('plus', 14), 'Create project')));
    newDlg.replaceChildren(h('div', { class: 'modal__head' }, h('h2', { class: 'modal__title', id: 'new-title' }, 'Create New Project')), form);
    newDlg.showModal();
    name.focus();
  }

  /* The menu is position:fixed so scrolling table containers never clip it. */
  function placeMenu() {
    const m = document.querySelector('.kebab__menu');
    if (!m) return;
    const b = m.parentElement.querySelector('.kebab__btn').getBoundingClientRect();
    const w = m.offsetWidth;
    const hgt = m.offsetHeight;
    const top = b.bottom + 4 + hgt > window.innerHeight ? Math.max(4, b.top - hgt - 4) : b.bottom + 4;
    m.style.top = `${top}px`;
    m.style.left = `${Math.max(4, Math.min(b.right - w, window.innerWidth - w - 4))}px`;
  }
  $('#main').addEventListener('scroll', () => { if (menu) { menu = null; render(); } });
  window.addEventListener('resize', () => { if (menu) placeMenu(); });

  /* ------------------------------------------------------------------ render */

  function render() {
    const list = projectList();
    const closed = state.closed.map((id) => {
      const s = Store.projects().find((p) => p.id === id);
      return s ? { id, name: s.name } : null;
    }).filter(Boolean);
    UI.preserveFocus(() => {
      $('#page').replaceChildren(
        h('header', { class: 'pg__head' },
          h('span', { class: 'pg__icon' }, iconEl('folder')),
          h('div', { class: 'pg__titles' },
            h('h1', { class: 'pg__title' }, 'Projects & Workspaces'),
            h('p', { class: 'pg__sub' }, 'Manage your projects, repositories and running workspaces')),
          h('div', { class: 'pg__actions' },
            h('button', { type: 'button', class: 'btn', id: 'add-btn', 'aria-haspopup': 'dialog', on: { click: openAdd } }, iconEl('folder', 16), '+ Add Existing Folder'),
            h('button', { type: 'button', class: 'btn btn--emerald btn--solid', id: 'new-btn', 'aria-haspopup': 'dialog', on: { click: openNew } }, iconEl('plus', 16), 'Create New Project'))),
        h('div', { class: 'pcards' }, list.map(projectCard)),
        closed.length ? h('div', { class: 'closed card' }, h('span', { class: 'muted' }, 'Closed projects:'),
          closed.map((c) => h('button', { type: 'button', class: 'btn btn--sm', 'data-fk': `reopen-${c.id}`, on: { click: () => { state.closed = state.closed.filter((x) => x !== c.id); save(); toast(`${c.name} reopened`, 'emerald', 'folder'); render(); } } }, iconEl('plus', 12), `Reopen ${c.name}`))) : null,
        historyCard(list));
    });
    placeMenu();
    if (focusMenu) {
      focusMenu = false;
      const first = document.querySelector('.kebab__menu .kebab__item');
      if (first) first.focus();
    }
  }

  window.HerdrShell.registerCommands([
    { group: 'Projects', label: 'Add existing folder…', icon: 'folder', hint: 'project', run: openAdd },
    { group: 'Projects', label: 'Create new project…', icon: 'plus', hint: 'project', run: openNew },
    { group: 'Projects', label: 'Show all previous runs', icon: 'clock', hint: 'history', run: () => { showAll = true; render(); } },
  ]);

  Store.subscribe((reason) => { if (reason !== 'theme' && reason !== 'chat') render(); });
  render();
}());
