/* Screen 09 — Notifications / Attention Center. Real store attention items across projects, plus page-local
   alerts (blocked prompt, lead wake-ups) persisted under herdr.notifications.* */
(function () {
  'use strict';

  const Store = window.OrchDemoStore;
  const UI = window.HerdrUI;
  const { h, iconEl, toast } = UI;
  const $ = (sel, root = document) => root.querySelector(sel);

  const LS_LOCAL = 'herdr.notifications.local';
  const KINDS = {
    hold: { label: 'Risk Hold', chip: 'Risk Holds', tone: 'amber', icon: 'warning' },
    question: { label: 'Coder Question', chip: 'Questions', tone: 'cyan', icon: 'question' },
    failure: { label: 'Verification Failure', chip: 'Failures', tone: 'coral', icon: 'shieldalert' },
    blocked: { label: 'Permission Prompt Blocked', chip: 'Blocked Prompts', tone: 'coral', icon: 'shieldalert' },
    wakeup: { label: 'Unacknowledged Lead Wake-Up', chip: 'Lead Wake-ups', tone: 'violet', icon: 'pulse' },
  };
  const KIND_ORDER = ['question', 'hold', 'failure', 'blocked', 'wakeup'];
  const STATUSES = [['open', 'Open'], ['resolved', 'Resolved'], ['dismissed', 'Dismissed'], ['all', 'All']];

  const DETAIL = {
    'att-m4-hold': { stat: 'Deletes src/legacy_auth.rs and modifies migrations/0007_create_categories.sql (+1,620 lines).', pane: null },
  };
  const DIFF = [
    ['hunk', 'diff --git a/migrations/0007_create_categories.sql b/migrations/0007_create_categories.sql'],
    ['hunk', '@@ -0,0 +1,18 @@'],
    ['add', '+CREATE TABLE categories ('],
    ['add', '+  id          INTEGER PRIMARY KEY,'],
    ['add', '+  name        TEXT NOT NULL UNIQUE,'],
    ['add', '+  archived_at TEXT'],
    ['add', '+);'],
    ['hunk', 'diff --git a/src/legacy_auth.rs b/src/legacy_auth.rs'],
    ['del', 'deleted file mode 100644'],
    ['del', '-pub fn verify_legacy_token(token: &str) -> bool {'],
    ['del', '-    token.starts_with("v1.")'],
    ['del', '-}'],
  ];

  const at = (hh, mm) => { const d = new Date(); d.setHours(hh, mm, 0, 0); return d.toISOString(); };
  const LOCAL_DEFS = [
    {
      id: 'local-blocked-m3', project: 'herdr-orchestra', kind: 'blocked', milestone: 'M3', agent: 'coder-2', pane: '2-3',
      body: 'Agent pane blocked waiting for permission:', command: 'npm install zod ? [y/N]', created_at: at(23, 33),
      options: [
        { id: 'open', label: 'Open Pane in Herdr', icon: 'external', tone: 'neutral', keep: true },
        { id: 'allow', label: 'Allow Once', icon: 'check', tone: 'emerald' },
        { id: 'deny', label: 'Deny', icon: 'x', tone: 'coral' },
      ],
    },
    {
      id: 'local-wake-m5', project: 'streamforge', kind: 'wakeup', milestone: 'M5', agent: 'lead', pane: '1-1',
      body: 'Lead pane has not acknowledged READY event for M5 after 300s (Wake 2/3).', created_at: at(22, 20),
      options: [
        { id: 'rewake', label: 'Re-Wake Lead', icon: 'sync', tone: 'violet' },
        { id: 'dismiss', label: 'Dismiss', tone: 'neutral', dismiss: true },
      ],
    },
    {
      id: 'local-wake-m1', project: 'herdr-orchestra', kind: 'wakeup', milestone: 'M1', agent: 'lead', pane: '1-1',
      body: 'Lead acknowledged READY for M1 after Wake 1/3.', created_at: at(23, 20), initial: { status: 'resolved', label: 'Acknowledged by lead' },
      options: [{ id: 'rewake', label: 'Re-Wake Lead', icon: 'sync', tone: 'violet' }],
    },
  ];

  const state = { project: 'all', kind: 'all', status: 'open', q: '', unacked: false };
  const selected = new Set();
  const drafts = new Map();
  let local = readLocal();

  function readLocal() {
    try { return JSON.parse(localStorage.getItem(LS_LOCAL)) || {}; } catch (_) { return {}; }
  }
  function saveLocal() {
    try { localStorage.setItem(LS_LOCAL, JSON.stringify(local)); } catch (_) { /* storage unavailable */ }
  }

  /* ------------------------------------------------------------------ model */

  function collect() {
    const out = [];
    Store.projects().forEach((p) => {
      Store.data(p.id).attention.forEach((a) => out.push({
        key: `${p.id}:${a.id}`, id: a.id, project: p.id, kind: a.kind, status: a.status, milestone: a.milestone,
        agent: a.agent, body: a.body, options: a.options, created_at: a.created_at, resolution: a.resolution,
        source: 'store', stat: (DETAIL[a.id] || {}).stat, pane: null,
      }));
    });
    LOCAL_DEFS.forEach((d) => {
      const st = local[d.id] || d.initial || { status: 'open' };
      out.push({
        key: `${d.project}:${d.id}`, id: d.id, project: d.project, kind: d.kind, status: st.status, milestone: d.milestone,
        agent: d.agent, body: d.body, options: d.options, created_at: d.created_at, command: d.command, pane: d.pane,
        resolution: st.status === 'open' ? null : { label: st.label, at: st.at || d.created_at }, source: 'local',
      });
    });
    return out.sort((a, b) => (a.status === 'open' ? 0 : 1) - (b.status === 'open' ? 0 : 1) || b.created_at.localeCompare(a.created_at));
  }

  const projectName = (id) => (Store.projects().find((p) => p.id === id) || { name: id }).name;
  const matches = (it) => {
    if (state.project !== 'all' && it.project !== state.project) return false;
    if (state.kind !== 'all' && it.kind !== state.kind) return false;
    if (state.status !== 'all' && it.status !== state.status) return false;
    const q = state.q.trim().toLowerCase();
    if (!q) return true;
    const hay = [it.milestone, it.agent, it.project, it.body, it.command, it.stat, KINDS[it.kind].label, it.pane,
      ...it.options.map((o) => o.label)].filter(Boolean).join(' ').toLowerCase();
    return hay.includes(q);
  };

  /* ------------------------------------------------------------------ actions */

  function copyText(text) {
    const done = () => toast('Copied to clipboard', 'emerald', 'copy');
    const fail = () => toast(`Clipboard blocked. Copy manually: ${text}`, 'amber', 'warning');
    try {
      if (navigator.clipboard && navigator.clipboard.writeText) navigator.clipboard.writeText(text).then(done, fail);
      else fail();
    } catch (_) { fail(); }
  }

  function ensureProject(id) {
    if (id !== Store.project) { Store.actions.switchProject(id); toast(`Switched to ${projectName(id)}`, 'violet', 'folder'); }
  }

  function applyOption(it, opt, silent) {
    if (opt.keep) { toast(`Opening pane ${it.pane || ''} in Herdr`.trim(), 'cyan', 'external'); return true; }
    if (it.source === 'local') {
      local[it.id] = { status: opt.dismiss ? 'dismissed' : 'resolved', label: opt.label, at: new Date().toISOString() };
      saveLocal();
      selected.delete(it.key);
      if (!silent) toast(`${it.milestone}: ${opt.label}`, KINDS[it.kind].tone, 'check');
      render();
      return true;
    }
    ensureProject(it.project);
    const done = Store.actions.resolveAttention(it.id, opt.id);
    selected.delete(it.key);
    if (done && !silent) toast(`${it.milestone}: ${opt.label}`, KINDS[it.kind].tone, 'check');
    return !!done;
  }

  function sendAnswer(it) {
    const text = (drafts.get(it.key) || '').trim();
    if (!text) { toast('Type an answer first', 'amber', 'warning'); const f = document.querySelector(`[data-fk="ans-${it.key}"]`); if (f) f.focus(); return; }
    const opt = /hard|remov|delete/i.test(text) ? it.options.find((o) => o.id === 'hard')
      : /soft|hide|archiv|omit|include/i.test(text) ? it.options.find((o) => o.id === 'soft') : null;
    if (!opt) { toast('Answer must pick a side: mention soft-delete/archive or hard-delete/remove', 'amber', 'warning'); return; }
    ensureProject(it.project);
    Store.actions.sendChat(`Answer for ${it.agent} (${it.milestone}): ${text}`);
    drafts.delete(it.key);
    applyOption(it, opt, true);
    toast(`Answer relayed to ${it.agent}`, 'cyan', 'send');
  }

  const dlg = $('#nc-dialog');
  UI.enhanceDialog(dlg);
  $('#nc-dialog-x').replaceChildren(iconEl('x'));
  $('#nc-dialog-x').addEventListener('click', () => dlg.close());
  $('#nc-dialog-close').addEventListener('click', () => dlg.close());
  let dlgOpener = null;
  dlg.addEventListener('close', () => { if (dlgOpener && document.contains(dlgOpener)) dlgOpener.focus(); dlgOpener = null; });
  function openDialog(title, nodes) {
    dlgOpener = document.activeElement;
    $('#nc-dialog-title').textContent = title;
    $('#nc-dialog-body').replaceChildren(...nodes);
    if (!dlg.open) dlg.showModal();
  }

  function inspectDiff(it) {
    openDialog(`Diff for ${it.milestone}`, [
      h('p', { class: 'muted' }, 'Read-only preview of the held milestone against ', h('span', { class: 'mono' }, 'feature/categories'), '.'),
      h('pre', { class: 'nc-diff', tabindex: '0', 'aria-label': 'Diff preview' }, DIFF.map(([c, t]) => h('div', { class: c }, t))),
      h('p', { class: 'mono muted' }, '+1,620 -214 across 2 files'),
    ]);
  }
  function rejectHelp(it) {
    const row = (cmd) => h('div', { class: 'nc-copy' }, h('pre', null, h('code', null, cmd)),
      h('button', { type: 'button', class: 'btn btn--icon', 'aria-label': `Copy ${cmd}`, on: { click: () => copyText(cmd) } }, iconEl('copy')));
    openDialog(`Reject or pause ${it.milestone}`, [
      h('p', null, `Approving is the only action the API exposes for a hold. To reject ${it.milestone} or stop the run, use the CLI:`),
      row('orch pause'),
      row(`orch fix ${it.milestone} "split the migration from legacy_auth removal"`),
      h('p', { class: 'muted' }, 'The hold stays open here until it is approved from this page, the inbox, or the bell drawer.'),
    ]);
  }

  /* ------------------------------------------------------------------ rendering */

  function btn(it, opt, extra) {
    const solid = opt.tone === 'emerald' || extra === 'solid';
    const cls = opt.tone === 'neutral' ? 'btn' : `btn btn--${opt.tone}${solid ? ' btn--solid' : ''}`;
    return h('button', { type: 'button', class: cls, dataset: { fk: `act-${it.key}-${opt.id}` }, on: { click: () => applyOption(it, opt) } },
      opt.icon ? iconEl(opt.icon) : null, opt.label);
  }

  function card(it) {
    const k = KINDS[it.kind];
    const title = h('h3', { class: 'nc-card__title' },
      h('span', { class: 'mono' }, `[${projectName(it.project)} · ${it.milestone}${it.agent && it.agent !== 'lead' ? ` · ${it.agent}` : it.agent === 'lead' ? ' · Lead' : ''}]`),
      ' ', h('span', { class: 'kind' }, k.label));
    const check = h('input', {
      type: 'checkbox', class: 'nc-card__sel', 'aria-label': `Select ${it.milestone} ${k.label}`, dataset: { fk: `sel-${it.key}` },
      checked: selected.has(it.key) ? true : null,
      on: { change: (e) => { if (e.target.checked) selected.add(it.key); else selected.delete(it.key); renderBulk(); } },
    });
    const kids = [
      h('div', { class: 'nc-card__head' }, check, h('span', { class: 'nc-card__icon' }, iconEl(k.icon)), title,
        h('span', { class: `badge badge--${k.tone}` }, k.label.replace('Permission Prompt ', '').replace('Unacknowledged ', '').replace('Coder ', ''))),
      h('p', { class: 'nc-card__body' }, it.kind === 'hold' ? `Risk Hold Triggered: ${it.body}` : it.body),
    ];
    if (it.stat) kids.push(h('p', { class: 'nc-card__body mono' }, it.stat));
    if (it.command) kids.push(h('div', { class: 'nc-cmd' }, h('span', null, '$'), h('span', null, 'Run command: ', h('b', null, it.command))));
    kids.push(h('div', { class: 'nc-card__meta' },
      h('span', null, UI.fmtTime(it.created_at)),
      h('span', { class: 'mono' }, it.source === 'store' ? 'api/v1/attention' : 'local alert'),
      it.pane ? h('span', { class: 'mono' }, `pane ${it.pane}`) : null));

    const actions = [];
    if (it.kind === 'hold') {
      it.options.forEach((o) => actions.push(btn(it, o, 'solid')));
      actions.push(h('button', { type: 'button', class: 'btn', dataset: { fk: `diff-${it.key}` }, on: { click: () => inspectDiff(it) } }, iconEl('branch'), 'Inspect Diff'));
      actions.push(h('button', { type: 'button', class: 'btn btn--coral', dataset: { fk: `rej-${it.key}` }, on: { click: () => rejectHelp(it) } }, iconEl('stop'), 'Reject / Pause'));
    } else {
      it.options.forEach((o, i) => actions.push(btn(it, o, it.kind === 'question' && i === 0 ? 'solid' : null)));
      if (it.kind === 'failure') {
        actions.push(h('button', { type: 'button', class: 'btn', dataset: { fk: `pane-${it.key}` }, on: { click: () => toast(`Opening ${it.agent} pane in Herdr`, 'cyan', 'external') } }, iconEl('external'), 'Open Pane in Herdr'));
      }
    }
    kids.push(h('div', { class: 'nc-card__actions' }, actions));

    if (it.kind === 'question') {
      const input = h('input', {
        class: 'input', type: 'text', placeholder: 'Type your answer or additional context...', value: drafts.get(it.key) || '',
        'aria-label': `Answer for ${it.agent}`, dataset: { fk: `ans-${it.key}` },
        on: { input: (e) => drafts.set(it.key, e.target.value), keydown: (e) => { if (e.key === 'Enter') { e.preventDefault(); sendAnswer(it); } } },
      });
      kids.push(h('div', { class: 'nc-answer' }, input,
        h('button', { type: 'button', class: 'btn btn--cyan', dataset: { fk: `send-${it.key}` }, on: { click: () => sendAnswer(it) } }, iconEl('send'), 'Send Answer')));
    }
    return h('li', { class: `card card--tone nc-card tone-${k.tone}`, dataset: { key: it.key } }, kids);
  }

  function doneRow(it) {
    const k = KINDS[it.kind];
    const dismissed = it.status === 'dismissed';
    const label = it.resolution ? it.resolution.label : 'Dismissed';
    return h('li', { class: `card nc-done${dismissed ? ' nc-done--dismissed' : ''}`, dataset: { key: it.key } },
      iconEl(dismissed ? 'x' : 'checkcircle'),
      h('span', null, h('strong', { class: 'mono' }, `[${projectName(it.project)} · ${it.milestone}]`), ` ${k.label} — `, h('strong', null, dismissed && !it.resolution ? 'Dismissed (superseded by a new request)' : label)),
      h('span', { class: `badge${dismissed ? '' : ' badge--emerald'}` }, it.status),
      it.resolution && it.resolution.at ? h('span', { class: 'nc-done__when' }, UI.fmtTime(it.resolution.at)) : null);
  }

  function renderChips(all) {
    const open = all.filter((i) => i.status === 'open');
    const mk = (cur, key, label, count, set) => h('button', {
      type: 'button', class: 'fchip', 'aria-pressed': String(cur === key), dataset: { fk: `chip-${set}-${key}` },
      on: { click: () => { state[set] = key; render(); } },
    }, label, count === null ? null : h('span', { class: 'mono' }, `(${count})`));
    $('#project-chips').replaceChildren(
      mk(state.project, 'all', 'All Projects', open.length, 'project'),
      ...Store.projects().map((p) => mk(state.project, p.id, p.name, open.filter((i) => i.project === p.id).length, 'project')));
    $('#kind-chips').replaceChildren(
      mk(state.kind, 'all', 'All kinds', null, 'kind'),
      ...KIND_ORDER.map((k) => mk(state.kind, k, KINDS[k].chip, open.filter((i) => i.kind === k).length, 'kind')));
  }

  function renderTabs(all) {
    const scope = all.filter((i) => (state.project === 'all' || i.project === state.project) && (state.kind === 'all' || i.kind === state.kind));
    const tabs = $('#status-tabs');
    tabs.replaceChildren(...STATUSES.map(([key, label]) => {
      const n = key === 'all' ? scope.length : scope.filter((i) => i.status === key).length;
      const on = state.status === key;
      return h('button', {
        type: 'button', role: 'tab', id: `status-tab-${key}`, 'aria-selected': String(on), 'aria-controls': 'status-panel',
        tabindex: on ? '0' : '-1', dataset: { fk: `tab-${key}`, status: key },
        on: { click: () => { state.status = key; render(); } },
      }, label, ' ', h('span', { class: 'tabcount' }, n));
    }));
    $('#status-panel').setAttribute('aria-labelledby', `status-tab-${state.status}`);
  }

  function renderList(all) {
    const rows = all.filter(matches);
    const list = $('#nc-list');
    if (!rows.length) {
      const msg = state.status === 'open' && !state.q && state.kind === 'all' && state.project === 'all'
        ? 'All clear. Nothing needs your attention.' : 'No notifications match these filters.';
      list.replaceChildren(h('li', { class: 'card nc-empty' }, iconEl('inbox'), h('span', null, msg),
        h('button', { type: 'button', class: 'btn btn--sm', dataset: { fk: 'reset-filters' }, on: { click: resetFilters } }, 'Reset filters')));
      return;
    }
    list.replaceChildren(...rows.map((it) => (it.status === 'open' ? card(it) : doneRow(it))));
  }

  function renderSide(all) {
    const open = all.filter((i) => i.status === 'open');
    $('#sum-title').replaceChildren(iconEl('bell'), `Open by kind (${open.length})`);
    $('#sum-list').replaceChildren(...KIND_ORDER.map((k) => {
      const n = open.filter((i) => i.kind === k).length;
      return h('li', null, h('button', {
        type: 'button', dataset: { fk: `sum-${k}` },
        on: { click: () => { state.kind = k; state.status = 'open'; render(); } },
      }, h('span', { class: `dot dot--${KINDS[k].tone}`, 'aria-hidden': 'true' }), KINDS[k].chip, h('span', { class: 'n' }, n)));
    }));

    const evs = [];
    Store.projects().forEach((p) => Store.data(p.id).events.forEach((e) => evs.push({ ...e, project: p.id })));
    const shown = evs.filter((e) => !state.unacked || !e.acked)
      .filter((e) => state.project === 'all' || e.project === state.project)
      .sort((a, b) => b.ts.localeCompare(a.ts));
    $('#hist-title').replaceChildren(iconEl('clock'), `Event history (${shown.length})`);
    $('#hist-list').replaceChildren(...(shown.length ? shown.map((e) => h('li', { class: `hist${e.acked ? '' : ' hist--unacked'}` },
      h('div', { class: 'hist__top' }, h('span', { class: 'badge badge--mono' }, e.type),
        e.milestones && e.milestones.length ? h('span', { class: 'mono muted' }, e.milestones.join(' ')) : null),
      h('p', { class: 'hist__msg' }, e.message),
      h('div', { class: 'hist__foot' }, h('span', { class: 'mono' }, projectName(e.project)), h('span', null, UI.fmtTime(e.ts)),
        e.acked ? h('span', { class: 'text-emerald' }, 'acknowledged')
          : h('button', {
            type: 'button', class: 'btn btn--sm', dataset: { fk: `ack-${e.project}-${e.seq}` },
            on: { click: () => { ensureProject(e.project); Store.actions.ackEvent(e.seq); } },
          }, iconEl('check', 13), 'Acknowledge')))) : [h('li', { class: 'muted' }, 'No events to show.')]));
  }

  function renderBulk() {
    const all = collect();
    [...selected].forEach((k) => { const it = all.find((i) => i.key === k); if (!it || it.status !== 'open') selected.delete(k); });
    const bar = $('#bulk-bar');
    bar.hidden = selected.size === 0;
    $('#bulk-count').textContent = `${selected.size} selected`;
    $('#bulk-apply').replaceChildren(iconEl('check'), 'Apply primary action');
    const holds = all.filter((i) => i.status === 'open' && i.kind === 'hold').length;
    const ah = $('#approve-holds');
    ah.replaceChildren(iconEl('warning'), `Approve all holds (${holds})`);
    ah.disabled = holds === 0;
  }

  function renderHead(all) {
    const open = all.filter((i) => i.status === 'open').length;
    $('#nc-title').textContent = `Attention Center (${open} Unresolved)`;
  }

  function render() {
    const all = collect();
    UI.keepFocus($('#main'), $('#status-panel'), () => UI.preserveFocus(() => {
      renderHead(all); renderChips(all); renderTabs(all); renderList(all); renderSide(all); renderBulk();
    }));
  }

  function resetFilters() {
    Object.assign(state, { project: 'all', kind: 'all', status: 'open', q: '', unacked: false });
    $('#nc-search').value = '';
    $('#unacked-only').checked = false;
    render();
  }

  /* ------------------------------------------------------------------ bulk + wiring */

  function runBulk(items, label) {
    const origin = Store.project;
    let n = 0;
    items.forEach((it) => {
      const cur = collect().find((i) => i.key === it.key);
      if (!cur || cur.status !== 'open') return;
      const opt = cur.options.find((o) => !o.keep) || cur.options[0];
      if (applyOption(cur, opt, true)) n += 1;
    });
    if (Store.project !== origin) Store.actions.switchProject(origin);
    toast(`${label}: ${n} item${n === 1 ? '' : 's'}`, 'emerald', 'check');
    render();
  }

  $('#nc-icon').replaceChildren(iconEl('bell'));
  $('#search-icon').replaceChildren(iconEl('search', 15));
  $('#restore-local').replaceChildren(iconEl('sync'), 'Restore local alerts');
  $('#restore-local').addEventListener('click', () => { local = {}; saveLocal(); toast('Local alerts restored', 'cyan', 'sync'); render(); });
  $('#approve-holds').addEventListener('click', () => runBulk(collect().filter((i) => i.status === 'open' && i.kind === 'hold'), 'Holds approved'));
  $('#bulk-apply').addEventListener('click', () => runBulk(collect().filter((i) => selected.has(i.key)), 'Resolved'));
  $('#bulk-clear').addEventListener('click', () => { selected.clear(); render(); });
  $('#nc-search').addEventListener('input', (e) => { state.q = e.target.value; render(); });
  $('#unacked-only').addEventListener('change', (e) => { state.unacked = e.target.checked; render(); });

  $('#status-tabs').addEventListener('keydown', (e) => {
    const keys = STATUSES.map((s) => s[0]);
    let i = keys.indexOf(state.status);
    if (e.key === 'ArrowRight') i = (i + 1) % keys.length;
    else if (e.key === 'ArrowLeft') i = (i - 1 + keys.length) % keys.length;
    else if (e.key === 'Home') i = 0;
    else if (e.key === 'End') i = keys.length - 1;
    else return;
    e.preventDefault();
    state.status = keys[i];
    render();
    $(`#status-tab-${state.status}`).focus();
  });

  window.HerdrShell.registerCommands([
    { group: 'Attention', label: 'Attention: show open items', icon: 'bell', hint: 'open', run: () => { state.status = 'open'; render(); } },
    { group: 'Attention', label: 'Attention: show resolved items', icon: 'check', hint: 'resolved', run: () => { state.status = 'resolved'; render(); } },
    { group: 'Attention', label: 'Attention: approve all risk holds', icon: 'warning', hint: 'holds', run: () => runBulk(collect().filter((i) => i.status === 'open' && i.kind === 'hold'), 'Holds approved') },
    { group: 'Attention', label: 'Attention: search notifications', icon: 'search', hint: 'focus', run: () => $('#nc-search').focus() },
    { group: 'Attention', label: 'Attention: reset filters', icon: 'sync', hint: 'filters', run: resetFilters },
  ]);

  Store.subscribe((reason) => { if (reason !== 'theme') render(); });
  render();
}());
