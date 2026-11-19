/* Screen 02 — Workflows (Plan). DAG / list view, milestone selection, contract inspector, plan settings. */
(function () {
  'use strict';

  const Store = window.OrchDemoStore;
  const UI = window.HerdrUI;
  const { h, iconEl, toast } = UI;
  const $ = (sel, root = document) => root.querySelector(sel);

  const TIER_TONE = { small: 'neutral', medium: 'amber', high: 'coral' };
  const STATE = {
    merged: { label: 'Merged', tone: 'emerald' },
    coding: { label: 'Coding', tone: 'cyan' },
    verifying: { label: 'Verifying', tone: 'amber' },
    failed: { label: 'Failed', tone: 'coral' },
    pending: { label: 'Pending', tone: 'neutral' },
  };

  let selected = 'M2';
  let view = 'dag';
  let showDeps = true;

  document.querySelectorAll('[data-icon]').forEach((n) => n.replaceWith(iconEl(n.dataset.icon)));

  const ms = () => Store.workflow().milestones;
  const byId = (id) => ms().find((m) => m.id === id);

  /* ------------------------------------------------------------------ small builders */

  function stateIcon(state, small) {
    const name = { merged: 'check', coding: 'spinner', verifying: 'spinner', failed: 'warning', pending: 'circle' }[state] || 'circle';
    const ic = iconEl(name);
    if (name === 'spinner') ic.classList.add('spin');
    return h('span', { class: `sicon sicon--${state}${small ? ' sicon--sm' : ''}`, role: 'img', 'aria-label': (STATE[state] || STATE.pending).label }, ic);
  }

  const badgeCls = (tone) => (tone === 'neutral' ? 'badge' : `badge badge--${tone}`);

  function stateBadge(state) {
    const st = STATE[state] || STATE.pending;
    return h('span', { class: badgeCls(st.tone) }, st.label, state === 'merged' ? ' ✓' : null);
  }

  const tierBadge = (tier, prefix) => h('span', { class: badgeCls(TIER_TONE[tier]) }, prefix ? `Tier: ${tier}` : tier);

  const KW = /\b(?:pub|trait|async|fn|impl|struct|let|CREATE|TABLE|PRIMARY|KEY|NOT|NULL|UNIQUE|DEFAULT|GET|POST|PUT|DELETE)\b/;
  const TY = /\b(?:Result|Vec|Uuid|UUID|TEXT|TIMESTAMPTZ|Category\w*|CreateCategory|UpdateCategory)\b/;

  /* Tiny syntax colouring; every token is a text node, nothing is parsed as HTML. */
  function highlight(line) {
    const cm = line.search(/(\/\/|--)/);
    const code = cm >= 0 ? line.slice(0, cm) : line;
    const comment = cm >= 0 ? line.slice(cm) : '';
    const parts = [];
    const re = new RegExp(`${KW.source}|${TY.source}`, 'g');
    const kwOnly = new RegExp(`^${KW.source}$`);
    let last = 0;
    let m;
    while ((m = re.exec(code))) {
      if (m.index > last) parts.push(code.slice(last, m.index));
      parts.push(h('span', { class: kwOnly.test(m[0]) ? 'tok-kw' : 'tok-ty' }, m[0]));
      last = m.index + m[0].length;
    }
    if (last < code.length) parts.push(code.slice(last));
    if (comment) parts.push(h('span', { class: 'tok-cm' }, comment));
    return parts;
  }

  /* ------------------------------------------------------------------ plan header */

  function renderHeader() {
    const wf = Store.workflow();
    $('#plan-icon').replaceChildren(iconEl('branch', 20));
    $('#plan-title').replaceChildren(h('span', { class: 'hash' }, '#'), h('span', { class: 'name' }, `Plan: ${wf.title}`));

    const chip = (field, icon, label, value, tone) => h('li', null,
      h('button', {
        type: 'button', class: `pchip pchip--${tone}`, dataset: { fk: `chip-${field}` }, 'aria-haspopup': 'dialog',
        title: `Edit ${label.toLowerCase()}`, on: { click: () => openEdit(field) },
      }, iconEl(icon), `${label}: `, h('span', { class: 'mono' }, value)));
    $('#plan-chips').replaceChildren(
      chip('base', 'branch', 'Base', wf.base, 'violet'),
      chip('branch', 'branch', 'Branch', wf.branch, 'cyan'),
      chip('check', 'terminal', 'Check', wf.check, 'cyan'),
      chip('isolation', 'layers', 'Isolation', wf.isolation, 'violet'),
      chip('scope', 'warning', 'Scope', wf.scope, wf.scope === 'off' ? 'muted' : 'amber'),
      chip('risk_hold', 'shieldalert', 'Risk hold', wf.risk_hold ? 'on' : 'off', wf.risk_hold ? 'coral' : 'muted'),
      chip('commits', 'code', 'Commits', wf.commits, 'violet'));
    $('#start-btn').disabled = !ms().some((m) => m.included);
  }

  /* ------------------------------------------------------------------ DAG / list */

  function depthOf(m, memo = {}) {
    if (memo[m.id] !== undefined) return memo[m.id];
    memo[m.id] = 0;
    memo[m.id] = m.depends_on.length ? 1 + Math.max(...m.depends_on.map((d) => depthOf(byId(d), memo))) : 0;
    return memo[m.id];
  }

  function nodeEl(m) {
    return h('button', {
      type: 'button', class: `dnode dnode--${m.state}${m.included ? '' : ' dnode--excluded'}`,
      'aria-pressed': String(m.id === selected), dataset: { id: m.id, fk: `node-${m.id}` },
      on: { click: () => select(m.id) },
    },
      stateIcon(m.state),
      h('span', null,
        h('span', { class: 'dnode__title' }, `${m.id}: ${m.title}`),
        h('span', { class: 'dnode__badges' },
          stateBadge(m.state),
          m.agent ? h('span', { class: 'badge badge--mono' }, iconEl('user', 12), m.agent) : null,
          tierBadge(m.tier, true),
          m.included ? null : h('span', { class: 'badge' }, 'Excluded'))));
  }

  function renderDag() {
    const body = $('#dag-body');
    if (view === 'list') {
      body.replaceChildren(h('div', { class: 'dag__list' }, h('table', { class: 'table' },
        h('thead', null, h('tr', null, ['ID', 'Milestone', 'Tier', 'State', 'Agent', 'Depends on'].map((c) => h('th', { scope: 'col' }, c)))),
        h('tbody', null, ms().map((m) => h('tr', { class: m.included ? '' : 'muted' },
          h('td', null, h('button', {
            type: 'button', class: 'btn btn--sm mono', 'aria-pressed': String(m.id === selected), dataset: { fk: `row-${m.id}` },
            on: { click: () => select(m.id) },
          }, m.id)),
          h('td', null, m.title),
          h('td', null, tierBadge(m.tier)),
          h('td', null, stateBadge(m.state)),
          h('td', { class: 'mono' }, m.agent || '—'),
          h('td', { class: 'mono' }, m.depends_on.length ? m.depends_on.join(', ') : '—')))))));
      return;
    }
    const cols = [];
    const memo = {};
    ms().forEach((m) => { const d = depthOf(m, memo); (cols[d] = cols[d] || []).push(m); });
    const tpl = document.createElement('template');
    tpl.innerHTML = '<svg class="dag__edges" aria-hidden="true" focusable="false"></svg>';
    const edges = tpl.content.firstElementChild;
    body.replaceChildren(edges, h('div', { class: 'dag__cols' }, cols.map((c) => h('div', { class: 'dag__col' }, c.map(nodeEl)))));
    drawEdges();
  }

  /* Orthogonal connectors with rounded elbows, computed from the laid-out node rectangles. */
  function drawEdges() {
    const body = $('#dag-body');
    const svg = $('.dag__edges', body);
    if (!svg) return;
    if (!showDeps) { svg.innerHTML = ''; return; }
    const origin = body.getBoundingClientRect();
    const sx = body.scrollLeft;
    const pos = (id) => {
      const n = $(`.dnode[data-id="${id}"]`, body);
      if (!n) return null;
      const r = n.getBoundingClientRect();
      return { l: r.left - origin.left + sx, r: r.right - origin.left + sx, y: r.top - origin.top + r.height / 2 };
    };
    let out = '';
    ms().forEach((m) => {
      const to = pos(m.id);
      m.depends_on.forEach((dep) => {
        const from = pos(dep);
        if (!from || !to) return;
        const hot = m.id === selected || dep === selected;
        const cls = hot ? ' class="on"' : '';
        const x1 = from.r; const y1 = from.y; const x2 = to.l - 8; const y2 = to.y;
        const xm = x1 + (to.l - x1) / 2;
        const dy = y2 - y1;
        let d;
        if (Math.abs(dy) < 2) d = `M${x1} ${y1} H${x2}`;
        else {
          const r = Math.min(10, Math.abs(dy) / 2);
          const k = Math.sign(dy);
          d = `M${x1} ${y1} H${xm - r} Q${xm} ${y1} ${xm} ${y1 + k * r} V${y2 - k * r} Q${xm} ${y2} ${xm + r} ${y2} H${x2}`;
        }
        out += `<path${cls} d="${d}"/><polygon${cls} points="${x2},${y2 - 5} ${x2 + 8},${y2} ${x2},${y2 + 5}"/>`;
      });
    });
    svg.innerHTML = out;
  }

  /* ------------------------------------------------------------------ milestones list */

  function renderMilestones() {
    $('#ms-title').replaceChildren(iconEl('layers'), `Milestones (${ms().length})`);
    $('#ms-rows').replaceChildren(...ms().map((m) => {
      const cb = h('input', {
        type: 'checkbox', 'aria-label': `Include ${m.id} in the run`, dataset: { fk: `inc-${m.id}` },
        on: { change: () => Store.actions.toggleMilestone(m.id) },
      });
      cb.checked = m.included;
      return h('li', { class: `mrow${m.id === selected ? ' mrow--sel' : ''}` },
        cb,
        h('button', {
          type: 'button', class: 'mrow__main', 'aria-current': m.id === selected ? 'true' : null, dataset: { fk: `ms-${m.id}` },
          on: { click: () => select(m.id) },
        },
          h('span', { class: 'mrow__id' }, m.id),
          h('span', null,
            h('span', { class: 'mrow__title' }, m.title),
            h('span', { class: 'mrow__why' }, 'Why separate: ', m.why,
              showDeps && m.depends_on.length ? h('span', null, ' · needs ', h('span', { class: 'mono' }, m.depends_on.join(' + '))) : null)),
          tierBadge(m.tier),
          h('span', { class: 'mrow__state' }, stateIcon(m.state, true))));
    }));
  }

  /* ------------------------------------------------------------------ detail */

  function renderDetail() {
    const m = byId(selected) || ms()[0];
    const st = STATE[m.state] || STATE.pending;
    const chips = (list) => list.map((t) => h('span', { class: 'path-chip' }, t));
    const doneCount = m.criteria.filter((c) => c.done).length;
    const busy = m.state === 'coding' || m.state === 'verifying' ? iconEl('spinner', 13) : null;
    if (busy) busy.classList.add('spin');

    const facts = h('dl', { class: 'detail__facts' },
      iconEl('target'), h('dt', null, 'Goal'), h('dd', null, m.goal),
      iconEl('help'), h('dt', null, 'Why separate'), h('dd', null, m.why),
      iconEl('package'), h('dt', null, 'Needs'), h('dd', null, chips(m.needs)),
      iconEl('file'), h('dt', null, 'Likely files'), h('dd', null, chips(m.files)));

    const criteria = h('div', { class: 'ac' }, m.criteria.map((c) => h('button', {
      type: 'button', class: 'ac__btn', 'aria-pressed': String(c.done), dataset: { fk: `ac-${m.id}-${c.id}` },
      title: c.done ? 'Mark as not met' : 'Mark as met',
      on: { click: () => Store.actions.toggleCriterion(m.id, c.id) },
    }, iconEl(c.done ? 'checkcircle' : 'circle'), h('span', null, h('b', null, `${c.id}:`), c.text))));

    const code = h('pre', { class: 'code', tabindex: '0', 'aria-label': `${m.id} interfaces` }, m.interfaces.map((line, i) =>
      h('span', { class: 'code__line' }, h('span', { class: 'code__no' }, i + 1), h('span', null, highlight(line)))));

    $('#detail').replaceChildren(
      h('div', { class: 'detail__head' },
        h('span', { class: 'detail__id' }, m.id),
        h('h2', { class: 'detail__title', id: 'detail-title' }, m.title),
        h('span', { class: badgeCls(st.tone) }, busy ? busy : null, st.label)),
      facts,
      h('div', { class: 'detail__cols' },
        h('div', { class: 'detail__col' },
          h('h3', { class: 'detail__h' }, iconEl('checkcircle'), `Acceptance Criteria (${m.criteria.length})`,
            h('span', { class: 'muted', style: 'font-weight:400;font-size:12px' }, `${doneCount} met`)),
          criteria),
        h('div', { class: 'detail__col' },
          h('h3', { class: 'detail__h detail__h--code' }, iconEl('code'), 'Interfaces (API Contract)'),
          code)));
  }

  /* ------------------------------------------------------------------ selection + view */

  function select(id) {
    if (!byId(id)) return;
    selected = id;
    renderAll();
    try { if (location.hash.slice(1) !== id) history.replaceState(null, '', `#${id}`); } catch (_) { /* file:// origins may refuse */ }
  }

  function setView(v) {
    view = v;
    $('#view-dag').setAttribute('aria-pressed', String(v === 'dag'));
    $('#view-list').setAttribute('aria-pressed', String(v === 'list'));
    renderDag();
  }
  $('#view-dag').addEventListener('click', () => setView('dag'));
  $('#view-list').addEventListener('click', () => setView('list'));

  $('#show-deps').addEventListener('change', (e) => { showDeps = e.target.checked; renderAll(); });

  /* arrow-key navigation through the milestone list */
  $('#ms-rows').addEventListener('keydown', (e) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const btn = e.target.closest('.mrow__main');
    if (!btn) return;
    e.preventDefault();
    const list = [...document.querySelectorAll('.mrow__main')];
    const next = list[list.indexOf(btn) + (e.key === 'ArrowDown' ? 1 : -1)];
    if (next) next.focus();
  });

  /* ------------------------------------------------------------------ actions */

  $('#sync-btn').replaceChildren(iconEl('sync'), 'Sync Plan');
  $('#edit-btn').replaceChildren(iconEl('pencil'), 'Edit Plan');
  $('#start-btn').replaceChildren(iconEl('play'), 'Start Run');

  $('#sync-btn').addEventListener('click', () => {
    const btn = $('#sync-btn');
    btn.disabled = true;
    btn.firstElementChild.classList.add('spin');
    setTimeout(() => {
      Store.actions.syncPlan();
      btn.disabled = false;
      btn.firstElementChild.classList.remove('spin');
      toast('Plan synced. No changes found in the plan file.', 'cyan', 'sync');
    }, 800);
  });

  $('#start-btn').addEventListener('click', () => {
    const included = ms().filter((m) => m.included);
    if (!included.length) { toast('Select at least one milestone to run.', 'amber', 'warning'); return; }
    if (!Store.actions.startRun()) {
      toast(`Run #${Store.run().id} is already live. Open the Execution Monitor to follow it.`, 'cyan', 'clock');
      return;
    }
    toast(`Run #${Store.run().id} started with ${included.length} milestone${included.length === 1 ? '' : 's'}. Opening Execution Monitor…`, 'emerald', 'play');
    setTimeout(() => { window.location.href = '03-execution-monitor.html'; }, 900);
  });

  /* edit dialog */
  const dialog = $('#edit-dialog');
  const form = $('#edit-form');
  document.querySelectorAll('#edit-dialog [data-icon]').forEach((n) => n.replaceWith(iconEl(n.dataset.icon)));
  UI.enhanceDialog(dialog);

  function openEdit(field) {
    const wf = Store.workflow();
    form.elements.base.value = wf.base;
    form.elements.branch.value = wf.branch;
    form.elements.check.value = wf.check;
    form.elements.isolation.value = wf.isolation;
    form.elements.scope.value = wf.scope;
    form.elements.commits.value = wf.commits;
    form.elements.risk_hold.checked = wf.risk_hold;
    if (!dialog.open) dialog.showModal();
    const target = form.elements[field] || form.elements.base;
    target.focus();
  }
  $('#edit-btn').addEventListener('click', () => openEdit('base'));
  $('#edit-close').addEventListener('click', () => dialog.close());
  $('#edit-cancel').addEventListener('click', () => dialog.close());
  form.addEventListener('submit', (e) => {
    e.preventDefault();
    const f = form.elements;
    Store.actions.updateWorkflow({
      base: f.base.value.trim(), branch: f.branch.value.trim(), check: f.check.value.trim(),
      isolation: f.isolation.value, scope: f.scope.value, commits: f.commits.value, risk_hold: f.risk_hold.checked,
    });
    dialog.close();
    toast('Plan settings saved', 'emerald', 'check');
  });

  window.HerdrShell.registerCommands([
    { group: 'Plan', label: 'Sync plan', icon: 'sync', hint: 'plan', run: () => $('#sync-btn').click() },
    { group: 'Plan', label: 'Edit plan settings…', icon: 'pencil', hint: 'plan', run: () => openEdit('base') },
    { group: 'Plan', label: 'Start run', icon: 'play', hint: 'run', run: () => $('#start-btn').click() },
    { group: 'Plan', label: 'Show DAG view', icon: 'dag', hint: 'view', run: () => setView('dag') },
    { group: 'Plan', label: 'Show list view', icon: 'list', hint: 'view', run: () => setView('list') },
  ]);

  /* ------------------------------------------------------------------ boot */

  function renderAll() {
    UI.preserveFocus(() => {
      renderHeader();
      renderDag();
      renderMilestones();
      renderDetail();
    });
  }

  function fromHash() {
    const id = location.hash.slice(1);
    if (id && byId(id)) { selected = id; renderAll(); }
  }

  const initial = location.hash.slice(1);
  if (initial && byId(initial)) selected = initial;
  else if (!byId(selected)) selected = ms()[0].id;

  Store.subscribe((reason) => {
    if (reason === 'project' && !byId(selected)) selected = ms()[0].id;
    if (reason !== 'theme' && reason !== 'chat' && reason !== 'attention') renderAll();
  });
  window.addEventListener('hashchange', fromHash);
  new ResizeObserver(() => drawEdges()).observe($('#dag-body'));
  renderAll();
}());
