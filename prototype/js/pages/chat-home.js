/* Screen 01 — Chat / Home. Renders the lead chat, plan card, attention inbox and agents from HerdrStore. */
(function () {
  'use strict';

  const Store = window.HerdrStore;
  const UI = window.HerdrUI;
  const { h, iconEl, toast } = UI;
  const $ = (sel) => document.querySelector(sel);

  const TIER_TONE = { small: 'emerald', medium: 'amber', high: 'coral' };
  const STATE_TONE = { pending: 'neutral', queued: 'cyan', coding: 'cyan', review: 'violet', held: 'amber', merged: 'emerald', failed: 'coral' };
  const LEAD_STATE = {
    waiting_for_go: { label: 'Waiting for Go', icon: 'pause', tone: 'violet' },
    running: { label: 'Running', icon: 'bolt', tone: 'emerald' },
    revising: { label: 'Revising plan…', icon: 'pencil', tone: 'amber' },
    planning: { label: 'Planning…', icon: 'layers', tone: 'cyan' },
  };
  const AGENT_BADGE = { coding: ['cyan', 'Coding'], reviewing: ['violet', 'Reviewing'], idle: [null, 'Idle'] };

  const expanded = new Set();
  let changesOpen = false;
  let changesDraft = '';

  /* static icons */
  $('#plus-icon').replaceWith(iconEl('plus', 18));
  $('#send-icon').replaceWith(iconEl('send', 18));
  $('#request-x').replaceWith(iconEl('x'));

  /* ------------------------------------------------------------------ header */

  function renderHeader() {
    const d = Store.data();
    const lead = d.status.lead;
    $('#lead-avatar').replaceChildren(iconEl('robot'));
    $('#lead-title').replaceChildren('Lead: ', h('span', { class: 'mono' }, `${lead.name} (${lead.agent}: ${lead.model})`));
    const st = LEAD_STATE[lead.state] || LEAD_STATE.running;
    const badge = $('#lead-status');
    badge.className = `badge badge--${st.tone}`;
    badge.replaceChildren(iconEl(st.icon, 13), `Status: ${st.label}`);
  }

  /* ------------------------------------------------------------------ plan card */

  function planCard(plan, tasks) {
    const byId = Object.fromEntries(tasks.map((t) => [t.milestone, t]));
    const chips = h('div', { class: 'plan__chips' },
      h('span', { class: 'pchip' }, iconEl('layers'), `${plan.milestones.length} Milestone${plan.milestones.length === 1 ? '' : 's'}`),
      h('span', { class: 'pchip pchip--violet' }, iconEl('branch'), 'Isolation: ', h('span', { class: 'mono' }, plan.isolation)),
      h('span', { class: 'pchip pchip--cyan' }, iconEl('bolt'), 'Max Parallel: ', h('span', { class: 'mono' }, plan.max_parallel)));

    const rows = plan.milestones.map((m) => {
      const t = byId[m.id];
      const showState = plan.approved && t;
      return h('li', { class: 'ms' },
        iconEl('branch'),
        h('span', { class: 'ms__id' }, m.id),
        h('span', { class: 'ms__title' }, m.title),
        showState ? h('span', { class: `ms__state tone-${STATE_TONE[t.state] || 'neutral'}` }, h('span', { class: 'dot', style: 'background:var(--c)' }), t.state) : null,
        h('span', { class: `badge badge--${TIER_TONE[m.tier]}`, title: `Model tier: ${m.tier}` }, m.tier));
    });

    const card = h('section', { class: 'card plan', 'aria-label': `Plan: ${plan.title}` },
      h('div', { class: 'plan__head' }, iconEl('doc'), h('h3', { class: 'plan__title' }, `Plan: ${plan.title}`)),
      chips,
      h('ul', { class: 'ms-list' }, rows));

    if (plan.approved) {
      card.append(h('div', { class: 'plan__actions' },
        h('span', { class: 'plan__done' }, iconEl('check'), 'Approved · autopilot is driving'),
        h('a', { class: 'plan__done', href: '02-workflows-plan.html', style: 'font-weight:500' }, 'View in Workflows (Plan)')));
      return card;
    }

    const goBtn = h('button', { type: 'button', class: 'btn btn--emerald btn--solid', id: 'go-btn', on: { click: () => { Store.actions.approvePlan(); toast('Plan approved — autopilot started', 'emerald', 'play'); } } },
      iconEl('play'), 'Approve & Start (Go)');
    const askBtn = h('button', {
      type: 'button', class: 'btn', 'aria-expanded': String(changesOpen), 'aria-controls': 'changes-box',
      on: { click: () => { changesOpen = !changesOpen; renderThread(); if (changesOpen) $('#changes-text').focus(); } },
    }, iconEl('pencil'), 'Ask for Changes');
    card.append(h('div', { class: 'plan__actions' }, goBtn, askBtn));

    if (changesOpen) {
      const ta = h('textarea', {
        class: 'textarea', id: 'changes-text', rows: '3', 'aria-label': 'Describe the changes you want to the plan',
        placeholder: 'e.g. Split M2 into list/read and write endpoints',
        on: { input: (e) => { changesDraft = e.target.value; } },
      });
      ta.value = changesDraft;
      card.append(h('form', {
        class: 'changes', id: 'changes-box',
        on: {
          submit: (e) => {
            e.preventDefault();
            if (!changesDraft.trim()) { ta.focus(); return; }
            const text = changesDraft;
            changesOpen = false; changesDraft = '';
            Store.actions.requestPlanChanges(text);
          },
          keydown: (e) => { if (e.key === 'Escape') { e.stopPropagation(); changesOpen = false; renderThread(); $('#go-btn') && $('#go-btn').focus(); } },
        },
      }, ta, h('div', { class: 'changes__row' },
        h('button', { type: 'submit', class: 'btn btn--cyan btn--sm' }, 'Send to lead'),
        h('button', { type: 'button', class: 'btn btn--ghost btn--sm', on: { click: () => { changesOpen = false; renderThread(); } } }, 'Cancel'))));
    }
    return card;
  }

  /* ------------------------------------------------------------------ thread */

  function messageEl(m, d) {
    const isUser = m.role === 'user';
    const avatar = isUser
      ? h('span', { class: 'avatar' }, 'AN')
      : h('span', { class: 'avatar avatar--lead' }, iconEl('robot'));
    const meta = h('div', { class: 'msg__meta' },
      h('span', { class: 'msg__author' }, m.author),
      isUser ? null : h('span', { class: 'badge badge--violet badge--mono' }, 'AI'),
      h('span', { class: 'msg__time' }, UI.fmtTime(m.ts)));
    const body = h('div', { class: 'msg__body' }, h('p', { class: 'msg__text' }, m.text));
    if (m.attachment && m.attachment.type === 'plan') {
      if (m.attachment.plan_id === d.plan.id) body.append(planCard(d.plan, d.tasks));
      else body.append(h('div', { class: 'card plan--old' }, iconEl('doc'), 'Earlier plan, superseded by a newer one below.'));
    }
    return h('article', { class: `msg msg--${m.role}` }, avatar, h('div', null, meta, body));
  }

  function renderThread() {
    const d = Store.data();
    const el = $('#thread');
    const nearBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
    const prevCount = el.querySelectorAll('.msg').length;
    const nodes = d.chat.map((m) => messageEl(m, d));
    if (d.status.lead.typing) {
      nodes.push(h('article', { class: 'msg msg--lead', 'aria-label': 'lead is typing' },
        h('span', { class: 'avatar avatar--lead' }, iconEl('robot')),
        h('div', null, h('div', { class: 'msg__meta' }, h('span', { class: 'msg__author' }, 'lead'), h('span', { class: 'msg__time' }, 'typing…')),
          h('span', { class: 'typing', 'aria-hidden': 'true' }, h('i'), h('i'), h('i')))));
    }
    el.replaceChildren(...nodes);
    if (nearBottom || el.querySelectorAll('.msg').length > prevCount) el.scrollTop = el.scrollHeight;
  }

  /* ------------------------------------------------------------------ inbox */

  function renderInbox() {
    const d = Store.data();
    const open = d.attention.filter((a) => a.status === 'open');
    const resolved = d.attention.filter((a) => a.status === 'resolved');
    $('#inbox-title').replaceChildren(iconEl('bell'), `Attention Inbox (${open.length})`);
    const nodes = open.map(UI.attentionCard).concat(resolved.map(UI.attentionCard));
    if (!open.length) {
      nodes.unshift(h('li', { class: 'inbox__empty' }, iconEl('check'), h('strong', null, 'All clear'), h('span', null, 'Nothing needs your attention right now.')));
    }
    $('#inbox-list').replaceChildren(...nodes);
  }

  /* ------------------------------------------------------------------ agents */

  function renderAgents() {
    const d = Store.data();
    $('#agents-title').replaceChildren(iconEl('users'), 'Agents at Work');
    const items = d.agents.map((a) => {
      const [tone, label] = AGENT_BADGE[a.state] || [null, a.state];
      const open = expanded.has(a.id);
      const logId = `log-${a.id}`;
      const lines = a.output.map(Store.util.stripAnsi).join('\n');
      const head = h('button', {
        type: 'button', class: 'agent__head', 'aria-expanded': String(open), 'aria-controls': logId,
        on: { click: () => { if (expanded.has(a.id)) expanded.delete(a.id); else expanded.add(a.id); renderAgents(); const b = document.querySelector(`[aria-controls="${logId}"]`); if (b) b.focus(); } },
      },
        h('span', { class: `dot ${a.state === 'idle' ? 'dot--idle' : 'dot--emerald'}`, 'aria-hidden': 'true' }),
        h('span', { class: 'agent__icon' }, iconEl(a.role === 'reviewer' ? 'shield' : 'code', 18)),
        h('span', null,
          h('span', { class: 'agent__name' }, a.id, h('span', { class: 'mono' }, `(${a.cli}: ${a.model})`)),
          h('span', { class: 'agent__detail', style: 'display:block' }, a.detail)),
        h('span', { class: `badge ${tone ? `badge--${tone}` : ''}` }, a.milestone ? `${label} ${a.milestone}` : label),
        iconEl('chevron', 14).cloneNode(true));
      head.lastElementChild.classList.add('agent__chev');

      const row = h('li', { class: `agent agent--${a.role}` }, head);
      if (open) {
        row.append(h('div', { class: 'agent__log', id: logId },
          h('pre', { class: 'agent__pre', tabindex: '0', 'aria-label': `${a.id} recent output` }, lines),
          h('div', { class: 'agent__foot' },
            h('span', null, `read-only · ANSI stripped · pane `, h('span', { class: 'mono' }, a.pane)),
            h('button', {
              type: 'button', class: 'btn btn--sm',
              on: { click: () => { const r = Store.actions.openInHerdr(a.id); toast(`Opening pane ${r.pane} in Herdr (demo)`, 'cyan', 'external'); } },
            }, iconEl('external'), 'Open in Herdr'))));
      }
      return row;
    });
    $('#agents-list').replaceChildren(...items);
  }

  /* ------------------------------------------------------------------ composer + request dialog */

  $('#composer').addEventListener('submit', (e) => {
    e.preventDefault();
    const input = $('#chat-input');
    const text = input.value.trim();
    if (!text) { input.focus(); return; }
    input.value = '';
    Store.actions.sendChat(text);
    input.focus();
  });

  const dialog = $('#request-dialog');
  const form = $('#request-form');
  UI.enhanceDialog(dialog);
  const openRequest = () => { if (!dialog.open) dialog.showModal(); };
  $('#new-request').addEventListener('click', openRequest);
  $('#request-close').addEventListener('click', () => dialog.close());
  $('#request-cancel').addEventListener('click', () => dialog.close());
  dialog.addEventListener('close', () => form.reset());
  form.addEventListener('submit', (e) => {
    e.preventDefault();
    const f = new FormData(form);
    Store.actions.submitFeatureRequest({
      title: String(f.get('title')).trim(),
      description: String(f.get('description')).trim(),
      acceptance: String(f.get('acceptance')).trim(),
      milestones: f.get('milestones'),
      max_parallel: f.get('max_parallel'),
      isolation: f.get('isolation'),
      tier: f.get('tier'),
      migrations: f.get('migrations') === 'on',
    });
    dialog.close();
    toast('Feature request sent to lead', 'violet', 'send');
  });

  window.HerdrShell.registerCommands([
    { group: 'Chat', label: 'New feature request…', icon: 'plus', hint: 'form', run: openRequest },
    { group: 'Chat', label: 'Focus message box', icon: 'chat', hint: 'lead', run: () => $('#chat-input').focus() },
    {
      group: 'Chat', label: 'Approve & Start (Go)', icon: 'play', hint: 'plan',
      run: () => { if (!Store.data().plan.approved) { Store.actions.approvePlan(); toast('Plan approved — autopilot started', 'emerald', 'play'); } else toast('Plan already approved', 'cyan', 'check'); },
    },
  ]);

  /* ------------------------------------------------------------------ boot */

  function render(reason) {
    renderHeader();
    if (reason !== 'theme') {
      renderThread();
      UI.keepFocus($('#inbox'), $('#inbox'), renderInbox);
      renderAgents();
    }
  }
  Store.subscribe(render);
  render();
}());
