/* Screen 03 — Execution Monitor. Run controls, agent cards, plain-text output tabs, lead event timeline. */
(function () {
  'use strict';

  const Store = window.OrchDemoStore;
  const UI = window.HerdrUI;
  const { h, iconEl, toast } = UI;
  const $ = (sel, root = document) => root.querySelector(sel);

  const STATE_TONE = { coding: 'cyan', verifying: 'amber', approved: 'emerald', failed: 'coral', idle: 'neutral' };
  const RUN_BADGE = { live: ['emerald', 'Live'], paused: ['amber', 'Paused'], stopped: ['coral', 'Stopped'] };
  const EVENT_TONE = { VerificationPassed: 'emerald', MergeCompleted: 'emerald', TaskReady: 'cyan', LeadWoken: 'violet', QuestionAsked: 'amber' };
  const ROW_ICON = { Task: 'doc', Worktree: 'branch', Check: 'checkcircle', Step: 'list', Result: 'checkcircle', Notes: 'doc' };

  const badgeCls = (tone) => (tone === 'neutral' ? 'badge' : `badge badge--${tone}`);
  let tab = null;
  let lastLogKey = '';
  let lastEventCount = 0;

  /* ------------------------------------------------------------------ helpers */

  const run = () => Store.run();

  function fmtDur(secs) {
    const hh = Math.floor(secs / 3600);
    const mm = Math.floor((secs % 3600) / 60);
    const ss = secs % 60;
    if (hh) return mm ? `${hh}h ${mm}m` : `${hh}h`;
    return `${mm}m ${String(ss).padStart(2, '0')}s`;
  }

  function eventName(type) {
    if (!/[._]/.test(type)) return type;
    return type.split(/[._]/).map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join('');
  }

  function eventTone(type) {
    const n = eventName(type);
    if (EVENT_TONE[n]) return EVENT_TONE[n];
    return /Failed|Stopped/.test(n) ? 'coral' : /Paused/.test(n) ? 'amber' : 'cyan';
  }

  async function copyText(text) {
    try { await navigator.clipboard.writeText(text); toast(`Copied ${text}`, 'cyan', 'copy'); }
    catch (_) { toast(`Copy is blocked in this context. Path: ${text}`, 'amber', 'warning'); }
  }

  /* ------------------------------------------------------------------ log highlighting (plain text only) */

  const TOKEN = /(?<agent>\b(?:coder-\d+|reviewer)\b \([^)]*\))|(?<tag>\[autopilot\])|(?<mark>\((?:new|updated)\))|(?<path>(?:\.\.\/)?[a-z][\w-]*(?:\/[\w.-]+)+|\/api\/[\w/-]+|\b[\w-]+\.(?:rs|tsx?|sql)\b)/g;

  function logText(text) {
    if (text.startsWith('$')) return [h('span', { class: 't-cmd' }, text)];
    const out = [];
    let last = 0;
    TOKEN.lastIndex = 0;
    let m;
    while ((m = TOKEN.exec(text))) {
      if (m.index > last) out.push(text.slice(last, m.index));
      const g = m.groups;
      const cls = g.agent ? 't-agent' : g.tag ? 't-tag' : g.mark ? (m[0] === '(new)' ? 't-new' : 't-upd') : 't-path';
      out.push(h('span', { class: cls }, m[0]));
      last = m.index + m[0].length;
    }
    if (last < text.length) out.push(text.slice(last));
    return out;
  }

  /* ------------------------------------------------------------------ header + progress */

  function renderHead() {
    const r = run();
    $('#mon-icon').replaceChildren(iconEl('cube'));
    $('#mon-title').textContent = `Execution Monitor — Run #${r.id}`;
    const [tone, label] = RUN_BADGE[r.state];
    const live = $('#mon-live');
    live.className = `badge badge--pill badge--${tone}`;
    live.replaceChildren(h('span', { class: `dot dot--${tone}${r.state === 'live' ? ' dot--pulse' : ''}`, 'aria-hidden': 'true' }), label);
    const started = new Date(r.started_at).toLocaleString('en-US', { month: 'short', day: 'numeric', year: 'numeric', hour: 'numeric', minute: '2-digit' });
    $('#mon-sub').replaceChildren('Plan: ', r.plan, ' · Branch: ', h('span', { class: 'mono' }, r.branch), ` · Started: ${started}`);

    const spin = iconEl(r.state === 'live' ? 'spinner' : 'checkcircle');
    if (r.state === 'live') spin.classList.add('spin');
    $('#prog-icon').replaceChildren(spin);
    $('#prog-text').textContent = `${r.percent}% (${r.merged}/${r.total} Merged, ${r.coding} Coding)`;
    const bar = $('#prog-bar');
    bar.className = `bar${r.state === 'live' ? '' : ` bar--${r.state}`}`;
    bar.setAttribute('aria-valuenow', String(r.percent));
    bar.setAttribute('aria-valuetext', `${r.percent} percent`);
    $('#prog-fill').style.width = `${r.percent}%`;
    $('#clock-icon').replaceChildren(iconEl('clock'));
    $('#budget').textContent = fmtDur(r.budget_s);
    updateElapsed();
  }

  function updateElapsed() { $('#elapsed').textContent = fmtDur(run().elapsed_s); }

  /* ------------------------------------------------------------------ agent cards */

  function rowEl(row) {
    const label = h('span', { class: 'arow__label' }, `${row.label}:`);
    let val;
    if (row.kind === 'copy') {
      val = h('span', { class: 'arow__val' }, h('span', { class: 'wt' }, h('span', null, row.value),
        h('button', { type: 'button', class: 'wt__copy', 'aria-label': `Copy ${row.value}`, title: 'Copy path', on: { click: () => copyText(row.value) } }, iconEl('copy'))));
    } else if (row.kind === 'check') {
      val = h('span', { class: `arow__val ${row.ok ? 'arow__val--ok' : 'arow__val--bad'}` }, iconEl(row.ok ? 'checkcircle' : 'warning'), row.value);
    } else if (row.kind === 'steps') {
      const kids = [];
      row.steps.forEach((s, i) => {
        if (i) kids.push(h('span', { class: 'step-arrow', 'aria-hidden': 'true' }, '→'));
        kids.push(h('span', { class: `step step--${s.state}` }, h('i', null, s.state === 'done' ? iconEl('check') : null), s.label));
      });
      val = h('div', { class: 'steps' }, kids);
    } else {
      val = h('span', { class: row.label === 'Task' ? 'arow__val mono' : 'arow__val' }, row.value);
    }
    return h('div', { class: `arow${row.kind === 'steps' ? ' arow--steps' : ''}` }, iconEl(ROW_ICON[row.label] || 'doc'), label, val);
  }

  function renderAgents() {
    $('#agents').replaceChildren(...run().agents.map((a) => {
      const tone = STATE_TONE[a.state] || 'neutral';
      const hasTab = run().tabs.some((t) => t.id === a.id);
      const content = [
        h('span', { class: `dot dot--${a.state === 'idle' ? 'idle' : a.state === 'failed' ? 'coral' : 'emerald'}`, 'aria-hidden': 'true' }),
        h('span', { class: `acard__icon${a.icon === 'shield' ? ' acard__icon--shield' : ''}` }, iconEl(a.icon, 18)),
        h('span', { class: 'acard__name' }, a.id, h('span', { class: 'muted' }, ' · '), h('span', { class: 'mono' }, `${a.cli}: ${a.model}`)),
        h('span', { class: `${badgeCls(tone)} badge--mono` }, a.badge),
      ];
      const head = hasTab
        ? h('button', {
          type: 'button', class: 'acard__head', dataset: { fk: `agent-${a.id}` }, title: `Show ${a.id} output`,
          'aria-label': `${a.id}, ${a.badge}. Show output`, on: { click: () => selectTab(a.id, true) },
        }, content)
        : h('div', { class: 'acard__head' }, content);
      return h('article', { class: 'card acard', 'aria-label': a.id }, head, h('div', { class: 'acard__rows' }, a.rows.map(rowEl)));
    }));
  }

  /* ------------------------------------------------------------------ output tabs + log */

  function renderTabs() {
    const tabs = run().tabs;
    if (!tabs.some((t) => t.id === tab)) tab = tabs[0].id;
    $('#tabs').replaceChildren(...tabs.map((t) => h('button', {
      type: 'button', class: 'tab', role: 'tab', id: `tab-${t.id}`, 'aria-selected': String(t.id === tab), 'aria-controls': 'log',
      tabindex: t.id === tab ? '0' : '-1', dataset: { fk: `tab-${t.id}`, id: t.id },
      on: { click: () => selectTab(t.id, true) },
    }, t.label)));
    $('#log').setAttribute('aria-labelledby', `tab-${tab}`);
  }

  function renderLog() {
    const el = $('#log');
    const lines = run().outputs[tab] || [];
    const key = `${Store.project}:${tab}`;
    const switched = key !== lastLogKey;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
    const pad = (n) => String(n).padStart(3, '0');
    const rows = lines.map((l, i) => {
      const text = Store.util.stripAnsi(l.text);
      const cls = text.trim().startsWith('✓') ? ' ll--ok' : text.trim().startsWith('✗') ? ' ll--bad' : '';
      return h('div', { class: `ll${cls}` },
        h('span', { class: 'll__no' }, pad(i + 1)),
        h('span', { class: 'll__ts' }, l.ts ? `[${l.ts}]` : ''),
        h('span', { class: 'll__tx' }, logText(text)));
    });
    rows.push(h('div', { class: 'll' }, h('span', { class: 'll__no' }, pad(lines.length + 1)), h('span'), h('span', { class: 'll__tx' })));
    el.replaceChildren(...rows);
    if (switched) el.scrollTop = 0;
    else if (near) el.scrollTop = el.scrollHeight;
    lastLogKey = key;
  }

  function selectTab(id, focusTab) {
    tab = id;
    renderTabs();
    renderLog();
    if (focusTab) $(`#tab-${id}`).focus();
  }

  $('#tabs').addEventListener('keydown', (e) => {
    const ids = run().tabs.map((t) => t.id);
    let i = ids.indexOf(tab);
    if (e.key === 'ArrowRight') i = (i + 1) % ids.length;
    else if (e.key === 'ArrowLeft') i = (i - 1 + ids.length) % ids.length;
    else if (e.key === 'Home') i = 0;
    else if (e.key === 'End') i = ids.length - 1;
    else return;
    e.preventDefault();
    selectTab(ids[i], true);
  });

  /* ------------------------------------------------------------------ timeline */

  function renderTimeline() {
    const d = Store.data();
    const events = d.events.slice(-8);
    const r = run();
    $('#tl-title').replaceChildren(iconEl('layers'), 'Lead Event Queue & Transition Timeline');
    const live = $('#tl-live');
    const [tone, label] = RUN_BADGE[r.state];
    live.className = `badge badge--pill badge--${tone}`;
    live.replaceChildren(h('span', { class: `dot dot--${tone}`, 'aria-hidden': 'true' }), `${label} (${d.events.length})`);

    const items = events.map((e) => {
      const name = eventName(e.type);
      const tone2 = eventTone(e.type);
      const t = new Date(e.ts);
      const clock = Store.clockFmt(t.getHours() * 3600 + t.getMinutes() * 60 + t.getSeconds());
      const att = e.attention_id ? d.attention.find((a) => a.id === e.attention_id) : null;
      const pending = att && att.status === 'open';
      const target = e.milestones.find((m) => /^M\d+$/.test(m));
      const href = target ? `02-workflows-plan.html#${target}` : '01-chat-home.html';

      const acts = [];
      if (att) {
        acts.push(pending
          ? h('span', { class: 'badge badge--amber' }, 'Pending')
          : h('span', { class: 'tl__done' }, iconEl('check'), att.status === 'resolved' ? 'Answered' : 'Dismissed'));
      } else if (e.acked) {
        acts.push(h('span', { class: 'tl__done' }, iconEl('check'), 'Acked'));
      } else {
        acts.push(h('button', {
          type: 'button', class: 'btn btn--emerald btn--sm', dataset: { fk: `ack-${e.seq}` }, 'aria-label': `Acknowledge ${name}`,
          on: { click: () => { Store.actions.ackEvent(e.seq); toast(`${name} acknowledged`, 'emerald', 'check'); } },
        }, 'Ack', iconEl('check', 13)));
      }
      acts.push(h('button', {
        type: 'button', class: 'btn btn--icon btn--sm', dataset: { fk: `replay-${e.seq}` }, 'aria-label': `Replay ${name} to lead`, title: 'Replay to lead',
        on: { click: () => { Store.actions.replayEvent(e.seq); toast(`Replayed ${name} to lead`, 'cyan', 'sync'); } },
      }, iconEl('sync')));
      acts.push(pending
        ? h('button', {
          type: 'button', class: 'btn btn--coral btn--icon btn--sm', dataset: { fk: `attn-${e.seq}` }, 'aria-label': 'Open the question in notifications', title: 'Needs your answer',
          on: { click: () => window.HerdrShell.openDrawer() },
        }, iconEl('warning'))
        : h('a', { class: 'btn btn--icon btn--sm', href, 'aria-label': `Open ${e.milestones[0] || 'lead'} details`, title: target ? `Open ${target} in Workflows` : 'Open lead chat' }, iconEl('external')));

      return h('li', { class: 'tl__item', style: `--c:var(--accent-${tone2})` },
        h('span', { class: `dot dot--${tone2}`, 'aria-hidden': 'true' }),
        h('span', { class: 'tl__time' }, clock),
        h('div', null,
          h('div', { class: 'tl__name' }, name, e.milestones.length ? h('span', { class: 'mono' }, e.milestones.join(', ')) : null),
          h('div', { class: 'tl__msg' }, e.message)),
        h('div', { class: 'tl__act' }, acts));
    });
    const list = $('#tl');
    list.replaceChildren(...(items.length ? items : [h('li', { class: 'tl__empty' }, 'No events yet.')]));
    if (d.events.length > lastEventCount && lastEventCount) list.scrollTop = list.scrollHeight;
    lastEventCount = d.events.length;
  }

  /* ------------------------------------------------------------------ controls */

  $('#pause-btn').replaceChildren(iconEl('pause'), 'Pause');
  $('#resume-btn').replaceChildren(iconEl('play'), 'Resume');
  $('#stop-btn').replaceChildren(iconEl('stop'), 'Stop');
  $('#herdr-btn').replaceChildren(iconEl('external'), 'Open in Herdr');
  $('#pane-btn').replaceChildren(iconEl('external', 14), 'Open Pane in Herdr');

  function pause() {
    if (run().state === 'live') { Store.actions.setRunState('paused'); toast('Run paused. Agents finish their current step.', 'amber', 'pause'); }
    else toast(run().state === 'paused' ? 'Run is already paused.' : 'Run has stopped.', 'cyan', 'clock');
  }
  function resume() {
    if (run().state === 'paused') { Store.actions.setRunState('live'); toast('Run resumed.', 'emerald', 'play'); }
    else toast(run().state === 'live' ? 'Run is already live.' : 'A stopped run cannot be resumed. Start a new run from Workflows.', 'cyan', 'clock');
  }
  const stopDialog = $('#stop-dialog');
  UI.enhanceDialog(stopDialog);
  function stop() {
    if (run().state === 'stopped') { toast('Run has already stopped.', 'cyan', 'clock'); return; }
    if (!stopDialog.open) stopDialog.showModal();
  }
  stopDialog.addEventListener('close', () => {
    if (stopDialog.returnValue === 'stop') { Store.actions.setRunState('stopped'); toast('Run stopped.', 'coral', 'stop'); }
    stopDialog.returnValue = '';
  });
  $('#pause-btn').addEventListener('click', pause);
  $('#resume-btn').addEventListener('click', resume);
  $('#stop-btn').addEventListener('click', stop);
  $('#herdr-btn').addEventListener('click', () => toast(`Opening run #${run().id} workspace in Herdr (demo)`, 'cyan', 'external'));
  $('#pane-btn').addEventListener('click', () => {
    const t = run().tabs.find((x) => x.id === tab);
    toast(`Opening pane ${t.pane} (${t.label}) in Herdr (demo)`, 'cyan', 'external');
  });

  window.HerdrShell.registerCommands([
    { group: 'Run', label: 'Pause run', icon: 'pause', hint: 'run', run: pause },
    { group: 'Run', label: 'Resume run', icon: 'play', hint: 'run', run: resume },
    { group: 'Run', label: 'Stop run…', icon: 'stop', hint: 'run', run: stop },
    { group: 'Run', label: 'Open run in Herdr', icon: 'external', hint: 'herdr', run: () => $('#herdr-btn').click() },
    { group: 'Run', label: 'Show autopilot.log', icon: 'doc', hint: 'output', run: () => selectTab('autopilot', true) },
  ]);

  /* ------------------------------------------------------------------ boot */

  function renderAll() {
    UI.preserveFocus(() => {
      renderHead();
      renderAgents();
      renderTabs();
      renderLog();
      renderTimeline();
    });
  }

  Store.subscribe((reason) => { if (reason !== 'theme' && reason !== 'chat') renderAll(); });
  setInterval(() => { Store.tickRun(); updateElapsed(); }, 1000);
  renderAll();
}());
