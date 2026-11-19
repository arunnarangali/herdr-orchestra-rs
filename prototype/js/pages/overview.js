/* Screen 04 — Overview. System & run command center: progress, lead state, workers, health, git, attention, events. */
(function () {
  'use strict';

  const Store = window.OrchDemoStore;
  const UI = window.HerdrUI;
  const { h, iconEl, toast } = UI;
  const $ = (sel, root = document) => root.querySelector(sel);

  const LAST_COMMIT = {
    'herdr-orchestra': { hash: 'a4f91c2', message: 'feat(auth): add JWT refresh rotation' },
    streamforge: { hash: 'c19be07', message: 'feat(ingest): bound the pipeline channel' },
  };
  const LEAD_STATE = {
    waiting_for_go: ['amber', 'Paused (Idle)', 'pause'],
    running: ['emerald', 'Running', 'play'],
    planning: ['cyan', 'Planning', 'spinner'],
    revising: ['cyan', 'Revising plan', 'spinner'],
  };
  const SCOPES = ['warn', 'block', 'off'];
  const EVENT_ICON = {
    MergeCompleted: ['checkcircle', 'emerald'], VerificationPassed: ['checkcircle', 'emerald'],
    LeadWoken: ['bolt', 'violet'], TaskDispatched: ['play', 'cyan'], TaskReady: ['play', 'cyan'],
    QuestionAsked: ['question', 'amber'], VerificationFailed: ['warning', 'coral'],
  };

  let onlyUnacked = false;
  let answering = null;

  const badgeCls = (tone) => (tone === 'neutral' ? 'badge' : `badge badge--${tone}`);
  const pad3 = (n) => String(n).padStart(3, '0');

  function eventName(type) {
    if (!/[._]/.test(type)) return type;
    return type.split(/[._]/).map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join('');
  }

  function fmtDur(secs) {
    const hh = Math.floor(secs / 3600);
    const mm = Math.floor((secs % 3600) / 60);
    return hh ? `${hh}h ${String(mm).padStart(2, '0')}m` : `${mm}m`;
  }

  function fmtStamp(iso) {
    const d = new Date(iso);
    const day = d.toLocaleDateString('en-US', { month: 'short', day: 'numeric' });
    const t = d.toLocaleTimeString('en-GB', { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
    return `${day}, ${t}`;
  }

  /* ------------------------------------------------------------------ derived data */

  function milestoneStates() {
    const d = Store.data();
    return Store.workflow().milestones.map((m) => {
      const t = d.tasks.find((x) => x.milestone === m.id);
      let state = m.state;
      if (t && t.state === 'held') state = 'held';
      else if (t && t.state === 'queued' && state === 'pending') state = 'pending';
      if (state === 'verifying' || state === 'review') state = 'coding';
      if (state === 'failed') state = 'failed';
      return { id: m.id, state };
    });
  }

  function counts() {
    const c = { merged: 0, coding: 0, held: 0, pending: 0, failed: 0 };
    milestoneStates().forEach((m) => { c[m.state in c ? m.state : 'pending'] += 1; });
    c.total = c.merged + c.coding + c.held + c.pending + c.failed;
    c.percent = c.total ? Math.round(((c.merged + c.coding * 0.5) / c.total) * 100) : 0;
    return c;
  }

  function workerFor(e) {
    const d = Store.data();
    const name = eventName(e.type);
    if (name === 'LeadWoken' || /^Plan/.test(name)) return 'lead';
    if (name === 'VerificationPassed') return 'reviewer';
    if (name === 'TaskReady' || /^Run/.test(name)) return 'autopilot';
    if (e.attention_id) {
      const a = d.attention.find((x) => x.id === e.attention_id);
      if (a && a.agent) return a.agent;
    }
    const t = d.tasks.find((x) => x.milestone === e.milestone);
    const wfm = Store.workflow().milestones.find((x) => x.id === e.milestone);
    if (name === 'MergeCompleted') return (wfm && wfm.agent) || 'coder-1';
    return (t && t.agent) || (wfm && wfm.agent) || 'autopilot';
  }

  /* ------------------------------------------------------------------ cards */

  function cardHead(icon, title, extra) {
    return h('header', { class: 'card__head' }, h('h2', { class: 'card__title' }, iconEl(icon), title), extra || null);
  }

  function metricRow(icon, label, value) {
    return h('div', { class: 'mrow' }, iconEl(icon), h('span', { class: 'mrow__k' }, label), h('span', { class: 'mrow__v' }, value));
  }

  function progressCard() {
    const c = counts();
    const seg = (n, cls) => (n ? h('i', { class: `pbar__seg pbar__seg--${cls}`, style: `flex:${n}` }) : null);
    const legend = (tone, n, label) => h('span', { class: 'legend__item' }, h('span', { class: `dot dot--${tone}`, 'aria-hidden': 'true' }), `${n} ${label}`);
    return h('section', { class: 'card ov-progress', 'aria-labelledby': 'ov-h-progress' },
      h('header', { class: 'card__head' }, h('h2', { class: 'card__title', id: 'ov-h-progress' }, iconEl('layers'), 'Overall Progress')),
      h('div', { class: 'ov-progress__body' },
        h('div', { class: 'ov-progress__top' },
          h('strong', { class: 'ov-progress__pct' }, `${c.percent}%`),
          h('div', { class: 'pbar', role: 'progressbar', 'aria-label': 'Overall progress', 'aria-valuemin': '0', 'aria-valuemax': '100', 'aria-valuenow': String(c.percent) },
            seg(c.merged, 'merged'), seg(c.coding, 'coding'), seg(c.held, 'held'), seg(c.failed, 'failed'), seg(c.pending, 'pending'))),
        h('div', { class: 'legend' },
          legend('emerald', c.merged, 'Merged'), legend('cyan', c.coding, 'Coding'), legend('amber', c.held, 'Held'),
          c.failed ? legend('coral', c.failed, 'Failed') : null, legend('idle', c.pending, 'Pending'))));
  }

  function leadCard() {
    const d = Store.data();
    const st = d.status;
    const [tone, label, icon] = LEAD_STATE[st.lead.state] || ['neutral', st.lead.state, 'clock'];
    const unread = d.events.filter((e) => !e.acked).length;
    const spin = iconEl(icon, 14);
    if (icon === 'spinner') spin.classList.add('spin');
    return h('section', { class: 'card', 'aria-labelledby': 'ov-h-lead' },
      h('header', { class: 'card__head' }, h('h2', { class: 'card__title', id: 'ov-h-lead' }, iconEl('robot'), 'Lead State')),
      h('div', { class: 'card__pad' },
        h('span', { class: `${badgeCls(tone)} badge--pill badge--lg` }, spin, label),
        metricRow('code', 'Lead Mode:', h('span', { class: 'mono text-violet' }, 'events')),
        metricRow('bell', 'Unread Events:', h('span', { class: `${badgeCls(unread ? 'violet' : 'neutral')} mono` }, String(unread)))));
  }

  function workersCard() {
    const d = Store.data();
    const r = Store.run();
    const slots = d.agents.length;
    const busy = d.agents.filter((a) => a.state !== 'idle').length;
    return h('section', { class: 'card', 'aria-labelledby': 'ov-h-workers' },
      h('header', { class: 'card__head' }, h('h2', { class: 'card__title', id: 'ov-h-workers' }, iconEl('users'), 'Active Workers')),
      h('div', { class: 'card__pad' },
        h('div', { class: 'slots' }, h('strong', null, `${busy} / ${slots}`), h('span', null, 'Slots Busy')),
        metricRow('layers', `${d.plan.isolation === 'worktree' ? 'Worktree' : 'Shared'} Isolation`, null),
        metricRow('clock', 'Budget:', h('span', null, h('strong', { class: 'mono' }, fmtDur(r.elapsed_s)), h('span', { class: 'muted' }, ' / '), h('span', { class: 'mono' }, fmtDur(r.budget_s))))));
  }

  function healthCard() {
    const hd = Store.data().status.herdr;
    const ok = hd.connected;
    return h('section', { class: 'card', 'aria-labelledby': 'ov-h-health' },
      h('header', { class: 'card__head' }, h('h2', { class: 'card__title', id: 'ov-h-health' }, iconEl('activity'), 'System Health')),
      h('div', { class: 'card__pad' },
        h('span', { class: `${badgeCls(ok ? 'emerald' : 'coral')} badge--pill badge--lg` }, h('span', { class: `dot dot--${ok ? 'emerald' : 'coral'}`, 'aria-hidden': 'true' }), ok ? 'Healthy' : 'Degraded'),
        metricRow('branch', `Herdr v${hd.version} ${ok ? 'Connected' : 'Disconnected'}`, null),
        metricRow('package', 'SQLite WAL OK', null)));
  }

  function gitRow(icon, label, value) {
    return h('div', { class: 'git__row' }, iconEl(icon), h('span', { class: 'git__k' }, label), h('span', { class: 'git__sep', 'aria-hidden': 'true' }, ':'), h('div', { class: 'git__v' }, value));
  }

  function gitCard() {
    const wf = Store.workflow();
    const commit = LAST_COMMIT[Store.project] || { hash: '0000000', message: 'no commits yet' };
    const fp = fingerprint();
    const toggleRisk = () => {
      Store.actions.updateWorkflow({ risk_hold: !wf.risk_hold });
      toast(`Risk hold ${!wf.risk_hold ? 'enabled' : 'disabled'}`, !wf.risk_hold ? 'coral' : 'cyan', 'shieldalert');
    };
    const cycleScope = () => {
      const next = SCOPES[(SCOPES.indexOf(wf.scope) + 1) % SCOPES.length];
      Store.actions.updateWorkflow({ scope: next });
      toast(`Scope check set to ${next}`, 'cyan', 'warning');
    };
    return h('section', { class: 'card', 'aria-labelledby': 'ov-h-git' },
      h('header', { class: 'card__head' }, h('h2', { class: 'card__title', id: 'ov-h-git' }, iconEl('branch'), 'Active Orchestration & Git')),
      h('div', { class: 'git' },
        gitRow('doc', 'Plan', h('a', { class: 'mono text-violet git__link', href: '02-workflows-plan.html' }, Store.data().plan.title)),
        gitRow('branch', 'Feature Branch', h('div', { class: 'git__split' },
          h('a', { class: 'mono text-cyan git__link', href: '02-workflows-plan.html' }, wf.branch),
          h('span', { class: 'badge badge--mono' }, 'Base: ', h('span', { class: 'text-primary' }, wf.base)))),
        gitRow('activity', 'Last Commit', h('div', { class: 'git__split git__split--wrap' },
          h('span', { class: 'badge badge--mono' }, commit.hash), h('span', { class: 'mono git__msg' }, commit.message))),
        gitRow('target', 'Setup Fingerprint', h('button', {
          type: 'button', class: `${badgeCls(fp.valid ? 'emerald' : 'amber')} badge--btn`, 'data-fk': 'ov-fp', title: 'Re-verify the setup fingerprint',
          on: { click: () => { fingerprint(true); toast('Setup fingerprint re-verified: valid', 'emerald', 'checkcircle'); renderAll(); } },
        }, iconEl(fp.valid ? 'checkcircle' : 'warning', 14), fp.valid ? 'valid' : 'stale')),
        gitRow('shield', 'Risk Hold', h('button', {
          type: 'button', class: `${badgeCls(wf.risk_hold ? 'coral' : 'neutral')} badge--btn mono`, 'data-fk': 'ov-risk', 'aria-pressed': String(!!wf.risk_hold),
          title: 'Toggle risk hold', on: { click: toggleRisk },
        }, wf.risk_hold ? 'ON' : 'OFF')),
        gitRow('warning', 'Scope', h('button', {
          type: 'button', class: `${badgeCls(wf.scope === 'off' ? 'neutral' : wf.scope === 'block' ? 'coral' : 'amber')} badge--btn mono`, 'data-fk': 'ov-scope',
          title: 'Cycle scope check: warn, block, off', on: { click: cycleScope },
        }, wf.scope))));
  }

  const fpState = {};
  function fingerprint(reset) {
    if (reset) fpState[Store.project] = true;
    return { valid: fpState[Store.project] !== false };
  }

  /* ------------------------------------------------------------------ attention */

  function attentionRow(item) {
    const tone = { hold: 'amber', question: 'cyan', failure: 'coral' }[item.kind] || 'neutral';
    const icon = { hold: 'warning', question: 'question', failure: 'warning' }[item.kind] || 'bell';
    const word = { hold: 'Held', question: 'Question', failure: 'Failed' }[item.kind] || 'Attention';
    const detail = item.kind === 'question'
      ? `${item.agent} asks: ${item.body}`
      : item.kind === 'hold' ? item.body.replace(/^The milestone \w+ is held for approval because it /, 'It ').replace(/^It modifies/, 'Touches') : item.body;
    let action;
    if (item.kind === 'hold') {
      const opt = item.options.find((o) => o.id === 'approve') || item.options[0];
      action = h('button', { type: 'button', class: `btn btn--sm btn--${tone}`, 'data-fk': `att-${item.id}`, on: { click: () => resolve(item, opt.id) } }, iconEl('check', 14), 'Approve');
    } else if (item.kind === 'question') {
      action = h('button', { type: 'button', class: `btn btn--sm btn--${tone}`, 'data-fk': `att-${item.id}`, 'aria-haspopup': 'dialog', on: { click: () => openAnswer(item) } }, iconEl('chat', 14), 'Answer');
    } else {
      const opt = item.options[0];
      action = h('button', { type: 'button', class: `btn btn--sm btn--${tone}`, 'data-fk': `att-${item.id}`, on: { click: () => resolve(item, opt.id) } }, iconEl(opt.icon || 'play', 14), opt.label);
    }
    return h('li', { class: `card card--tone tone-${tone} attn` },
      h('span', { class: 'attn__icon' }, iconEl(icon, 20)),
      h('span', { class: 'attn__text' },
        h('span', { class: `${badgeCls(tone)} mono attn__tag` }, `${item.milestone} ${word}`),
        h('span', { class: 'attn__body' }, `— ${detail}`)),
      action);
  }

  function resolve(item, optionId) {
    const done = Store.actions.resolveAttention(item.id, optionId);
    if (done) toast(`${item.milestone}: ${done.resolution.label}`, 'emerald', 'check');
  }

  function attentionCardEl() {
    const items = Store.openAttention();
    return h('section', { class: 'card attn-card', 'aria-labelledby': 'ov-h-attn' },
      h('header', { class: 'card__head' },
        h('h2', { class: 'card__title', id: 'ov-h-attn' }, iconEl('warning'), 'Needs Attention (Action Required)'),
        h('span', { class: 'badge badge--pill', 'aria-live': 'polite' }, `${items.length} item${items.length === 1 ? '' : 's'}`)),
      items.length
        ? h('ul', { class: 'attn-list' }, items.map(attentionRow))
        : h('div', { class: 'empty' }, iconEl('checkcircle', 28), h('strong', null, 'All clear'), h('span', { class: 'muted' }, 'No holds, questions or failures need you right now.')));
  }

  /* ------------------------------------------------------------------ events */

  function eventsCard() {
    const all = Store.data().events.slice().sort((a, b) => b.seq - a.seq).slice(0, 50);
    const rows = all.filter((e) => !onlyUnacked || !e.acked);
    const unacked = all.filter((e) => !e.acked).length;
    const filterBtn = (val, label) => h('button', {
      type: 'button', 'aria-pressed': String(onlyUnacked === val), 'data-fk': `ev-filter-${val}`,
      on: { click: () => { onlyUnacked = val; renderAll(); } },
    }, label);

    const body = rows.map((e) => {
      const name = eventName(e.type);
      const [icon, tone] = EVENT_ICON[name] || ['activity', /Failed|Stopped/.test(name) ? 'coral' : 'cyan'];
      const worker = workerFor(e);
      return h('tr', { class: e.acked ? 'is-acked' : null },
        h('td', { class: 'mono muted' }, pad3(e.seq)),
        h('td', { class: 'mono ev__ts' }, fmtStamp(e.ts)),
        h('td', null, h('span', { class: `ev__name text-${tone}` }, iconEl(icon, 18), h('span', { class: 'mono' }, name))),
        h('td', null, e.milestones && e.milestones.length ? h('span', { class: 'badge badge--mono' }, e.milestones.join(', ')) : h('span', { class: 'muted' }, '–')),
        h('td', { class: `mono ${worker === 'lead' || worker === 'reviewer' ? 'text-violet' : worker === 'autopilot' ? 'text-cyan' : 'text-violet'}` }, worker),
        h('td', { class: 'ev__detail' }, e.message),
        h('td', { class: 'ev__ack' }, e.acked
          ? h('span', { class: 'ev__done' }, iconEl('check', 14), 'Acked')
          : h('button', { type: 'button', class: 'btn btn--sm btn--emerald', 'data-fk': `ack-${e.seq}`, 'aria-label': `Acknowledge event ${pad3(e.seq)} ${name}`, on: { click: () => Store.actions.ackEvent(e.seq) } }, iconEl('check', 14), 'Ack')));
    });

    return h('section', { class: 'card ev', 'aria-labelledby': 'ov-h-events' },
      h('header', { class: 'card__head ev__head' },
        h('h2', { class: 'card__title', id: 'ov-h-events' }, iconEl('layers'), 'Recent Execution Events'),
        h('div', { class: 'seg', role: 'group', 'aria-label': 'Event filter' }, filterBtn(false, 'All'), filterBtn(true, `Unacked (${unacked})`)),
        h('button', {
          type: 'button', class: 'btn btn--sm', disabled: unacked === 0 || null, 'data-fk': 'ack-all',
          on: { click: () => { all.filter((e) => !e.acked).forEach((e) => Store.actions.ackEvent(e.seq)); toast('All events acknowledged', 'emerald', 'check'); } },
        }, iconEl('check', 14), 'Ack all'),
        h('span', { class: 'badge badge--lg' }, 'Last 50 events')),
      h('div', { class: 'ev__scroll' },
        h('table', { class: 'table ev__table' },
          h('thead', null, h('tr', null, ['#', 'Timestamp', 'Event', 'Milestone', 'Worker', 'Details', 'Ack'].map((c) => h('th', { scope: 'col' }, c)))),
          h('tbody', null, rows.length ? body : h('tr', null, h('td', { colspan: '7', class: 'empty-cell' }, 'Every event is acknowledged.'))))));
  }

  /* ------------------------------------------------------------------ answer dialog */

  const dialog = h('dialog', { class: 'modal', id: 'answer-dialog', closedby: 'any', 'aria-labelledby': 'answer-title' });
  document.body.append(dialog);
  UI.enhanceDialog(dialog);

  function openAnswer(item) {
    answering = item.id;
    dialog.replaceChildren(
      h('div', { class: 'modal__head' }, h('h2', { class: 'modal__title', id: 'answer-title' }, `${item.agent} asks (${item.milestone})`)),
      h('div', { class: 'modal__body' }, h('p', null, item.body), h('p', { class: 'muted' }, 'Your answer is relayed to the agent through the lead.')),
      h('form', { method: 'dialog', class: 'modal__foot' },
        h('button', { type: 'submit', class: 'btn', value: '' }, 'Cancel'),
        item.options.map((o, i) => h('button', { type: 'submit', class: o.tone === 'neutral' ? 'btn' : `btn btn--${o.tone || 'cyan'}`, value: o.id, autofocus: i === 0 || null }, o.icon ? iconEl(o.icon, 14) : null, o.label))));
    dialog.showModal();
  }

  dialog.addEventListener('close', () => {
    const val = dialog.returnValue;
    dialog.returnValue = '';
    const item = Store.data().attention.find((a) => a.id === answering);
    answering = null;
    if (item && val) resolve(item, val);
  });

  /* ------------------------------------------------------------------ render */

  function headEl() {
    return h('header', { class: 'pg__head' },
      h('span', { class: 'pg__icon' }, iconEl('grid')),
      h('div', null,
        h('h1', { class: 'pg__title' }, 'Overview'),
        h('p', { class: 'pg__sub' }, `System & Run Command Center — ${Store.data().status.project}`)));
  }

  function renderAll() {
    const key = document.activeElement && document.activeElement.dataset ? document.activeElement.dataset.fk : null;
    UI.preserveFocus(() => {
      $('#page').replaceChildren(
        headEl(),
        h('div', { class: 'ov-top' }, progressCard(), leadCard(), workersCard(), healthCard()),
        h('div', { class: 'ov-mid' }, gitCard(), attentionCardEl()),
        eventsCard());
    });
    if (key && !document.querySelector(`[data-fk="${key}"]`)) {
      const next = document.querySelector(key.startsWith('att-') ? '.attn .btn' : '.ev__ack .btn') || document.getElementById('main');
      next.focus();
    }
  }

  window.HerdrShell.registerCommands([
    { group: 'Overview', label: 'Acknowledge all events', icon: 'check', hint: 'events', run: () => { Store.data().events.filter((e) => !e.acked).forEach((e) => Store.actions.ackEvent(e.seq)); toast('All events acknowledged', 'emerald', 'check'); } },
    { group: 'Overview', label: 'Show only unacknowledged events', icon: 'list', hint: 'filter', run: () => { onlyUnacked = true; renderAll(); } },
    { group: 'Overview', label: 'Show all events', icon: 'list', hint: 'filter', run: () => { onlyUnacked = false; renderAll(); } },
    { group: 'Overview', label: 'Re-verify setup fingerprint', icon: 'target', hint: 'git', run: () => { fingerprint(true); toast('Setup fingerprint re-verified: valid', 'emerald', 'checkcircle'); renderAll(); } },
  ]);

  Store.subscribe((reason) => { if (reason !== 'theme' && reason !== 'chat') renderAll(); });
  renderAll();
}());
