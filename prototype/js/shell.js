/* Herdr Orchestra — shared shell: top bar, sidebar, status bar, Ctrl+K palette,
   theme switcher, notification drawer, toasts. Exposes window.HerdrUI and window.HerdrShell. */
(function (global) {
  'use strict';

  const Store = global.HerdrStore;

  /* ------------------------------------------------------------------ icons */

  const ICONS = {
    pulse: '<path d="M3 12h4l3-8 4 16 3-8h4"/>',
    grid: '<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/>',
    chat: '<path d="M21 15a2 2 0 0 1-2 2H8l-5 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>',
    folder: '<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>',
    robot: '<rect x="4" y="8" width="16" height="12" rx="3"/><circle cx="12" cy="4" r="1"/><path d="M12 5v3M9 13v2M15 13v2M2 13v3M22 13v3"/>',
    branch: '<circle cx="6" cy="5" r="2"/><circle cx="6" cy="19" r="2"/><circle cx="18" cy="8" r="2"/><path d="M6 7v10M18 10c0 4-6 3-12 7"/>',
    activity: '<path d="M22 12h-4l-3 9L9 3l-3 9H2"/>',
    chart: '<path d="M4 20V10M10 20V4M16 20v-8M22 20H2"/>',
    gear: '<path d="M12 8.5a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7z"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>',
    rocket: '<path d="M4.5 16.5c-1.5 1.3-2 5-2 5s3.7-.5 5-2c.7-.8.7-2.1-.1-2.9a2.2 2.2 0 0 0-2.9-.1z"/><path d="M12 15l-3-3a22 22 0 0 1 2-3.9A12.9 12.9 0 0 1 22 2c0 2.7-.8 7.5-6 11a22 22 0 0 1-4 2z"/><path d="M9 12H4s.6-3 2-4c1.6-1.1 5 0 5 0M12 15v5s3-.6 4-2c1.1-1.6 0-5 0-5"/>',
    help: '<circle cx="12" cy="12" r="10"/><path d="M9.1 9a3 3 0 0 1 5.8 1c0 2-3 3-3 3M12 17h.01"/>',
    bell: '<path d="M18 8a6 6 0 0 0-12 0c0 7-3 9-3 9h18s-3-2-3-9M13.7 21a2 2 0 0 1-3.4 0"/>',
    moon: '<path d="M21 12.8A9 9 0 1 1 11.2 3 7 7 0 0 0 21 12.8z"/>',
    sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
    search: '<circle cx="11" cy="11" r="7"/><path d="M21 21l-4.3-4.3"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    send: '<path d="M22 2L11 13M22 2l-7 20-4-9-9-4z"/>',
    play: '<path d="M6 4l14 8-14 8z"/>',
    pencil: '<path d="M12 20h9M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/>',
    check: '<path d="M20 6L9 17l-5-5"/>',
    warning: '<path d="M10.3 3.9L1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0zM12 9v4M12 17h.01"/>',
    question: '<circle cx="12" cy="12" r="10"/><path d="M9.1 9a3 3 0 0 1 5.8 1c0 2-3 3-3 3M12 17h.01"/>',
    trash: '<path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6M10 11v6M14 11v6"/>',
    code: '<path d="M16 18l6-6-6-6M8 6l-6 6 6 6"/>',
    shield: '<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/>',
    layers: '<path d="M12 2L2 7l10 5 10-5z"/><path d="M2 17l10 5 10-5M2 12l10 5 10-5"/>',
    bolt: '<path d="M13 2L3 14h9l-1 8 10-12h-9z"/>',
    users: '<path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M23 21v-2a4 4 0 0 0-3-3.9M16 3.1a4 4 0 0 1 0 7.8"/>',
    doc: '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6M8 13h8M8 17h5"/>',
    pause: '<path d="M8 5v14M16 5v14"/>',
    x: '<path d="M18 6L6 18M6 6l12 12"/>',
    menu: '<path d="M3 6h18M3 12h18M3 18h18"/>',
    cube: '<path d="M12 2l9 5v10l-9 5-9-5V7z"/><path d="M12 12l9-5M12 12L3 7M12 12v10"/>',
    external: '<path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6M15 3h6v6M10 14L21 3"/>',
    chevron: '<path d="M6 9l6 6 6-6"/>',
    sync: '<path d="M21 12a9 9 0 0 0-15.5-6.2L3 8M3 3v5h5M3 12a9 9 0 0 0 15.5 6.2L21 16M21 21v-5h-5"/>',
    list: '<path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/>',
    dag: '<circle cx="12" cy="5" r="2.2"/><circle cx="5" cy="19" r="2.2"/><circle cx="19" cy="19" r="2.2"/><path d="M12 7.2v4.3M12 11.5L6 17M12 11.5L18 17"/>',
    terminal: '<path d="M4 17l6-6-6-6M12 19h8"/>',
    clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
    stop: '<rect x="5" y="5" width="14" height="14" rx="2"/>',
    copy: '<rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h10a1 1 0 0 1 1 1v1"/>',
    target: '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="12" r="4"/><path d="M12 3v3M12 18v3M3 12h3M18 12h3"/>',
    file: '<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/>',
    package: '<path d="M21 8l-9-5-9 5v8l9 5 9-5z"/><path d="M3.3 7.5L12 12.5l8.7-5M12 22V12.5"/>',
    checkcircle: '<circle cx="12" cy="12" r="9"/><path d="M8 12.5l3 3 5-6"/>',
    circle: '<circle cx="12" cy="12" r="9"/>',
    spinner: '<circle cx="12" cy="12" r="9" stroke-dasharray="2.4 4.1"/>',
    shieldalert: '<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z"/><path d="M12 8v4M12 16h.01"/>',
    user: '<circle cx="12" cy="8" r="4"/><path d="M4 21v-1a6 6 0 0 1 6-6h4a6 6 0 0 1 6 6v1"/>',
    inbox: '<path d="M22 12h-6l-2 3h-4l-2-3H2"/><path d="M5.5 5.1L2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.5-6.9A2 2 0 0 0 16.8 4H7.2a2 2 0 0 0-1.7 1.1z"/>',
  };

  function svg(name, size) {
    const s = size ? ` style="width:${size}px;height:${size}px"` : '';
    return `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false"${s}>${ICONS[name] || ''}</svg>`;
  }

  /* ------------------------------------------------------------------ DOM helper */

  /* h('div', {class, dataset, on:{click}, html, aria-*}, ...children) — text children are never parsed as HTML. */
  function h(tag, props, ...kids) {
    const node = document.createElement(tag);
    Object.entries(props || {}).forEach(([k, v]) => {
      if (v === undefined || v === null || v === false) return;
      if (k === 'class') node.className = v;
      else if (k === 'html') node.innerHTML = v;
      else if (k === 'on') Object.entries(v).forEach(([ev, fn]) => node.addEventListener(ev, fn));
      else if (k === 'dataset') Object.assign(node.dataset, v);
      else if (v === true) node.setAttribute(k, '');
      else node.setAttribute(k, v);
    });
    kids.flat().forEach((c) => {
      if (c === null || c === undefined || c === false) return;
      node.append(c.nodeType ? c : document.createTextNode(String(c)));
    });
    return node;
  }

  function iconEl(name, size) {
    const t = document.createElement('template');
    t.innerHTML = svg(name, size);
    return t.content.firstElementChild;
  }

  /* ------------------------------------------------------------------ toasts */

  function toast(message, tone = 'cyan', iconName = 'check') {
    const host = document.getElementById('toasts');
    if (!host) return;
    const el = h('div', { class: `toast tone-${tone}` }, iconEl(iconName), h('span', null, message));
    host.append(el);
    setTimeout(() => el.remove(), 3600);
  }

  /* ------------------------------------------------------------------ dialogs */

  /* Light-dismiss fallback for browsers without <dialog closedby> (Safari). */
  function enhanceDialog(dialog) {
    if ('closedBy' in HTMLDialogElement.prototype || dialog.dataset.enhanced) return;
    dialog.dataset.enhanced = '1';
    const outside = (e) => {
      const r = dialog.getBoundingClientRect();
      return e.clientX < r.left || e.clientX > r.right || e.clientY < r.top || e.clientY > r.bottom;
    };
    let pressedOutside = false;
    dialog.addEventListener('pointerdown', (e) => { pressedOutside = outside(e); });
    dialog.addEventListener('pointerup', (e) => {
      if (dialog.getAttribute('closedby') === 'any' && pressedOutside && outside(e)) dialog.close();
    });
  }

  /* Re-render while keeping keyboard focus on the control marked with the same data-fk key. */
  function preserveFocus(renderFn) {
    const a = document.activeElement;
    const key = a && a.dataset ? a.dataset.fk : null;
    renderFn();
    if (!key) return;
    const n = document.querySelector(`[data-fk="${key}"]`);
    if (n && n !== document.activeElement) n.focus();
  }

  /* Run a re-render; if focus was inside `container` and the render removed it, move focus to `fallback`. */
  function keepFocus(container, fallback, renderFn) {
    const had = container.contains(document.activeElement) && document.activeElement !== container;
    renderFn();
    if (had && !container.contains(document.activeElement)) fallback.focus();
  }

  /* ------------------------------------------------------------------ attention card */

  /* Shared by the Attention Inbox and the notification drawer. */
  function attentionCard(item) {
    if (item.status !== 'open') {
      const label = item.resolution ? item.resolution.label : 'Dismissed';
      return h('li', { class: 'card att att--resolved' }, iconEl('check'),
        h('span', null, h('strong', { class: 'mono' }, item.milestone), ` · ${item.title.replace(/^.*?—\s*/, '')} — `, h('strong', null, label)));
    }
    const tone = { hold: 'amber', question: 'cyan', failure: 'coral' }[item.kind] || 'neutral';
    const iconName = { hold: 'warning', question: 'question', failure: 'warning' }[item.kind] || 'bell';
    return h('li', { class: `card card--tone att tone-${tone}`, dataset: { attention: item.id } },
      h('div', { class: 'att__head' },
        h('span', { class: 'att__icon' }, iconEl(iconName)),
        h('h3', { class: 'att__title' }, item.title),
        h('span', { class: `badge badge--mono badge--${tone === 'neutral' ? 'amber' : tone}` }, item.milestone)),
      h('p', { class: 'att__body' }, item.body),
      h('div', { class: 'att__actions' }, item.options.map((o) =>
        h('button', {
          type: 'button',
          class: o.tone === 'neutral' ? 'btn' : `btn btn--${o.tone}`,
          on: {
            click: () => {
              const done = Store.actions.resolveAttention(item.id, o.id);
              if (done) toast(`${item.milestone}: ${o.label}`, tone === 'neutral' ? 'cyan' : tone);
            },
          },
        }, o.icon ? iconEl(o.icon) : null, o.label))));
  }

  /* ------------------------------------------------------------------ nav model */

  const NAV = [
    { href: '04-overview.html', label: 'Overview', icon: 'grid', page: 'overview' },
    { href: '01-chat-home.html', label: 'Chat / Home', icon: 'chat', page: 'chat-home' },
    { href: '05-projects.html', label: 'Projects', icon: 'folder', page: 'projects' },
    { href: '06-agents-and-models.html', label: 'Agents', icon: 'robot', page: 'agents-and-models' },
    { href: '02-workflows-plan.html', label: 'Workflows (Plan)', icon: 'branch', page: 'workflows-plan' },
    { href: '03-execution-monitor.html', label: 'Execution Monitor', icon: 'activity', page: 'execution-monitor' },
    { href: '07-reports.html', label: 'Reports', icon: 'chart', page: 'reports' },
    { href: '08-settings.html', label: 'Settings', icon: 'gear', page: 'settings' },
    { href: '10-onboarding.html', label: 'Onboarding', icon: 'rocket', page: 'onboarding' },
    { href: '11-help-reference.html', label: 'Help', icon: 'help', page: 'help-reference' },
  ];
  const EXTRA_PAGES = [
    { href: '09-notifications-attention-center.html', label: 'Notifications / Attention Center', icon: 'bell', page: 'notifications-attention-center' },
  ];

  const page = document.body.dataset.page || '';
  const refs = {};
  const extraCommands = [];

  /* ------------------------------------------------------------------ top bar */

  function buildTopbar() {
    const el = document.getElementById('topbar');
    refs.tabs = h('div', { class: 'project-tabs', role: 'group', 'aria-label': 'Projects' });
    Store.projects().forEach((p) => {
      const btn = h('button', {
        type: 'button', class: 'project-tab', dataset: { project: p.id },
        on: { click: () => { Store.actions.switchProject(p.id); toast(`Switched to ${p.name}`, 'violet', 'folder'); } },
      }, h('span', { class: 'dot dot--emerald', 'aria-hidden': 'true' }), h('span', null, p.name), h('span', { class: 'project-tab__count', hidden: true }));
      refs.tabs.append(btn);
    });

    refs.branch = h('span', { class: 'chip chip--branch', title: 'Current branch' }, iconEl('branch'), h('span'));
    refs.herdr = h('span', { class: 'chip chip--status', role: 'status' }, h('span', { class: 'dot dot--emerald', 'aria-hidden': 'true' }), h('span'));
    refs.bellCount = h('span', { class: 'bell__count', 'aria-hidden': 'true' });
    refs.bell = h('button', {
      type: 'button', class: 'topbar__icon-btn', 'aria-haspopup': 'dialog',
      on: { click: openDrawer },
    }, iconEl('bell'), refs.bellCount);
    refs.theme = h('button', { type: 'button', class: 'topbar__icon-btn', on: { click: toggleTheme } });
    refs.navToggle = h('button', {
      type: 'button', class: 'topbar__icon-btn topbar__nav-toggle', 'aria-label': 'Toggle navigation',
      'aria-controls': 'sidebar', 'aria-expanded': 'false', on: { click: () => setNav(!document.body.classList.contains('nav-open')) },
    }, iconEl('menu'));

    el.append(
      refs.navToggle,
      h('a', { class: 'topbar__brand', href: '01-chat-home.html', 'aria-label': 'Herdr Orchestra home' }, iconEl('pulse'), h('span', { class: 'topbar__brand-name' }, 'Herdr Orchestra')),
      h('span', { class: 'topbar__sep', 'aria-hidden': 'true' }),
      refs.tabs,
      h('span', { class: 'topbar__spacer' }),
      refs.branch, refs.herdr,
      h('span', { class: 'topbar__sep', 'aria-hidden': 'true' }),
      refs.bell, refs.theme,
      h('button', { type: 'button', class: 'palette-btn', 'aria-haspopup': 'dialog', on: { click: openPalette } },
        iconEl('search'), h('span', null, 'Search'), h('kbd', null, 'Ctrl+K')),
    );
  }

  function updateTopbar() {
    const d = Store.data();
    const open = Store.openAttention().length;
    refs.tabs.querySelectorAll('.project-tab').forEach((btn) => {
      const info = Store.projects().find((p) => p.id === btn.dataset.project);
      const current = info.id === Store.project;
      btn.setAttribute('aria-current', current ? 'true' : 'false');
      const count = btn.querySelector('.project-tab__count');
      count.hidden = current || info.attention === 0;
      count.textContent = info.attention;
      if (!current && info.attention) btn.title = `${info.attention} item(s) need attention in ${info.name}`;
      else btn.removeAttribute('title');
    });
    refs.branch.lastElementChild.textContent = d.status.branch;
    const connected = d.status.herdr.connected;
    refs.herdr.dataset.connected = String(connected);
    refs.herdr.lastElementChild.textContent = `Herdr: ${connected ? 'Connected' : 'Disconnected'}`;
    refs.bellCount.textContent = open;
    refs.bellCount.hidden = open === 0;
    refs.bell.setAttribute('aria-label', `Notifications, ${open} need attention`);
    const light = Store.theme() === 'light';
    refs.theme.replaceChildren(iconEl(light ? 'sun' : 'moon'));
    refs.theme.setAttribute('aria-label', light ? 'Switch to dark theme' : 'Switch to light theme');
    refs.theme.setAttribute('aria-pressed', String(light));
  }

  function toggleTheme() { Store.actions.setTheme(Store.theme() === 'light' ? 'dark' : 'light'); }

  function setNav(open) {
    document.body.classList.toggle('nav-open', open);
    if (refs.navToggle) refs.navToggle.setAttribute('aria-expanded', String(open));
  }

  /* ------------------------------------------------------------------ sidebar */

  function buildSidebar() {
    const el = document.getElementById('sidebar');
    el.setAttribute('aria-label', 'Primary');
    const nav = h('nav', { class: 'side__nav', 'aria-label': 'Main' }, NAV.map((n) =>
      h('a', { class: 'nav__link', href: n.href, title: n.label, 'aria-current': n.page === page ? 'page' : null },
        iconEl(n.icon), h('span', null, n.label))));
    refs.driver = h('div', { class: 'driver-pill', role: 'status' }, iconEl('bolt'), h('span'));
    el.append(
      h('div', { class: 'side__brand' }, iconEl('cube'),
        h('div', { class: 'side__brand-text' }, h('strong', null, 'HERDR'), h('span', null, 'Orchestra'))),
      nav,
      h('div', { class: 'side__foot' }, refs.driver),
    );
    el.addEventListener('click', (e) => { if (e.target.closest('a')) setNav(false); });
  }

  function updateSidebar() {
    const drv = Store.data().status.driver;
    const state = drv.state.charAt(0).toUpperCase() + drv.state.slice(1);
    refs.driver.lastElementChild.textContent = `Driver: ${drv.command} · ${state}`;
    refs.driver.title = `${drv.command} (PID ${drv.pid})`;
  }

  /* ------------------------------------------------------------------ status bar */

  function buildStatusbar() {
    const el = document.getElementById('statusbar');
    refs.statusMs = h('span');
    refs.statusLead = h('span');
    el.append(
      h('span', { class: 'demo-badge', title: 'All data comes from js/demo-store.js. No server is contacted.' }, 'Demo Data Layer'),
      h('span', { class: 'status__item status__optional' }, h('span', { class: 'dot dot--emerald dot--pulse', 'aria-hidden': 'true' }), 'SSE /api/v1/events'),
      h('span', { class: 'status__item' }, refs.statusMs),
      h('span', { class: 'status__spacer' }),
      h('span', { class: 'status__item status__optional' }, refs.statusLead),
      h('span', { class: 'status__item status__optional' }, 'prototype v0.1'),
    );
  }

  function updateStatusbar() {
    const d = Store.data();
    const merged = d.tasks.filter((t) => t.state === 'merged').length;
    refs.statusMs.textContent = `milestones ${merged}/${d.tasks.length} merged`;
    refs.statusLead.textContent = `lead: ${d.status.lead.agent}/${d.status.lead.model}`;
  }

  /* ------------------------------------------------------------------ notification drawer */

  function buildDrawer() {
    refs.drawerList = h('ul', { class: 'drawer__body', style: 'padding:0;list-style:none' });
    refs.drawerOthers = h('div', { class: 'muted', style: 'display:grid;gap:6px' });
    refs.drawerClose = h('button', {
      type: 'button', class: 'btn btn--ghost btn--icon btn--sm', 'aria-label': 'Close notifications',
      on: { click: () => refs.drawer.close() },
    }, iconEl('x'));
    refs.drawer = h('dialog', { class: 'drawer', id: 'notif-drawer', closedby: 'any', 'aria-labelledby': 'notif-title' },
      h('div', { class: 'drawer__head' },
        h('h2', { class: 'drawer__title', id: 'notif-title' }, iconEl('bell'), 'Notifications'),
        refs.drawerClose),
      h('div', { class: 'drawer__body' }, refs.drawerList, refs.drawerOthers),
      h('div', { class: 'drawer__foot' },
        h('a', { class: 'btn btn--block', href: '09-notifications-attention-center.html' }, 'Open Attention Center', iconEl('external'))));
    document.body.append(refs.drawer);
    enhanceDialog(refs.drawer);
  }

  function updateDrawer() {
    keepFocus(refs.drawer, refs.drawerClose, renderDrawer);
  }

  function renderDrawer() {
    const d = Store.data();
    const items = [...d.attention].filter((a) => a.status !== 'dismissed')
      .sort((a, b) => (a.status === 'open' ? 0 : 1) - (b.status === 'open' ? 0 : 1));
    refs.drawerList.replaceChildren(...(items.length
      ? items.map(attentionCard)
      : [h('li', { class: 'muted', style: 'padding:24px;text-align:center' }, 'All clear. Nothing needs your attention.')]));
    const others = Store.projects().filter((p) => p.id !== Store.project && p.attention > 0);
    refs.drawerOthers.replaceChildren(...(others.length ? [
      h('h3', { class: 'label', style: 'font-size:11px;text-transform:uppercase;letter-spacing:.05em' }, 'Other projects'),
      ...others.map((p) => h('button', {
        type: 'button', class: 'btn btn--block',
        on: { click: () => Store.actions.switchProject(p.id) },
      }, h('span', { class: 'mono' }, p.name), h('span', { class: 'badge badge--coral' }, `${p.attention} open`))),
    ] : []));
  }

  function openDrawer() { if (!refs.drawer.open) refs.drawer.showModal(); }

  /* ------------------------------------------------------------------ command palette */

  function allCommands() {
    const cmds = [];
    NAV.concat(EXTRA_PAGES).forEach((n) => cmds.push({
      group: 'Go to', label: n.label, icon: n.icon, hint: n.href.slice(0, 2),
      run: () => { global.location.href = n.href; },
    }));
    Store.projects().forEach((p) => cmds.push({
      group: 'Projects', label: `Switch to ${p.name}`, icon: 'folder', hint: p.branch,
      run: () => { Store.actions.switchProject(p.id); toast(`Switched to ${p.name}`, 'violet', 'folder'); },
    }));
    cmds.push(
      { group: 'Actions', label: 'Toggle light / dark theme', icon: 'moon', hint: 'theme', run: toggleTheme },
      { group: 'Actions', label: 'Open notifications', icon: 'bell', hint: 'attention', run: openDrawer },
      { group: 'Actions', label: 'Reset demo data', icon: 'sync', hint: 'reset', run: () => { Store.actions.resetDemo(); toast('Demo data reset to the starting state', 'amber', 'sync'); } },
    );
    return cmds.concat(extraCommands);
  }

  function buildPalette() {
    refs.palInput = h('input', {
      class: 'palette__input', type: 'text', placeholder: 'Type a command or page…', autocomplete: 'off', spellcheck: 'false',
      role: 'combobox', 'aria-expanded': 'true', 'aria-controls': 'palette-list', 'aria-autocomplete': 'list', 'aria-label': 'Command palette',
    });
    refs.palList = h('div', { class: 'palette__list', id: 'palette-list', role: 'listbox', 'aria-label': 'Commands' });
    refs.palette = h('dialog', { class: 'modal palette', id: 'palette', closedby: 'any', 'aria-label': 'Command palette' },
      h('div', { class: 'palette__search' }, iconEl('search'), refs.palInput),
      refs.palList,
      h('div', { class: 'palette__foot' },
        h('span', null, h('kbd', null, '↑'), ' ', h('kbd', null, '↓'), ' navigate'),
        h('span', null, h('kbd', null, 'Enter'), ' run'),
        h('span', null, h('kbd', null, 'Esc'), ' close')));
    document.body.append(refs.palette);
    enhanceDialog(refs.palette);

    let shown = [];
    let active = 0;

    const setActive = (i) => {
      active = shown.length ? (i + shown.length) % shown.length : 0;
      refs.palList.querySelectorAll('[role="option"]').forEach((o, idx) => o.setAttribute('aria-selected', String(idx === active)));
      const cur = refs.palList.querySelectorAll('[role="option"]')[active];
      if (cur) { refs.palInput.setAttribute('aria-activedescendant', cur.id); cur.scrollIntoView({ block: 'nearest' }); }
      else refs.palInput.removeAttribute('aria-activedescendant');
    };

    const run = (cmd) => { refs.palette.close(); cmd.run(); };

    const render = () => {
      const q = refs.palInput.value.trim().toLowerCase();
      shown = allCommands().filter((c) => !q || `${c.group} ${c.label}`.toLowerCase().includes(q));
      refs.palList.replaceChildren();
      if (!shown.length) { refs.palList.append(h('div', { class: 'palette__empty' }, 'No matching commands')); setActive(0); return; }
      let group = '';
      shown.forEach((c, i) => {
        if (c.group !== group) { group = c.group; refs.palList.append(h('div', { class: 'palette__group', role: 'presentation' }, group)); }
        refs.palList.append(h('div', {
          class: 'palette__item', role: 'option', id: `pal-opt-${i}`, 'aria-selected': 'false',
          on: { click: () => run(c), pointermove: () => { if (active !== i) setActive(i); } },
        }, iconEl(c.icon), h('span', null, c.label), c.hint ? h('span', { class: 'palette__hint' }, c.hint) : null));
      });
      setActive(0);
    };

    refs.palInput.addEventListener('input', render);
    refs.palInput.addEventListener('keydown', (e) => {
      if (e.key === 'ArrowDown') { e.preventDefault(); setActive(active + 1); }
      else if (e.key === 'ArrowUp') { e.preventDefault(); setActive(active - 1); }
      else if (e.key === 'Enter') { e.preventDefault(); if (shown[active]) run(shown[active]); }
    });
    refs.palette.addEventListener('close', () => { refs.palInput.value = ''; });
    refs.paletteRender = render;
  }

  function openPalette() {
    if (refs.palette.open) return;
    refs.paletteRender();
    refs.palette.showModal();
    refs.palInput.focus();
  }

  /* ------------------------------------------------------------------ boot */

  function render() {
    updateTopbar();
    updateSidebar();
    updateStatusbar();
    updateDrawer();
  }

  function init() {
    const app = document.getElementById('app');
    if (!app) return;
    app.prepend(h('a', { class: 'skip-link', href: '#main' }, 'Skip to main content'));
    document.body.append(h('div', { id: 'toasts', role: 'status', 'aria-live': 'polite' }));

    buildTopbar();
    buildSidebar();
    buildStatusbar();
    buildDrawer();
    buildPalette();

    const current = NAV.concat(EXTRA_PAGES).find((n) => n.page === page);
    if (current) document.title = `${current.label} · Herdr Orchestra`;

    document.documentElement.dataset.theme = Store.theme();
    Store.subscribe(render);
    render();

    document.addEventListener('keydown', (e) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        if (refs.palette.open) refs.palette.close(); else openPalette();
      } else if (e.key === 'Escape' && document.body.classList.contains('nav-open')) {
        setNav(false);
        if (refs.navToggle) refs.navToggle.focus();
      }
    });
    global.addEventListener('resize', () => { if (global.innerWidth > 640) setNav(false); });
    document.querySelectorAll('dialog[closedby="any"]').forEach(enhanceDialog);
  }

  global.HerdrUI = { h, svg, iconEl, toast, enhanceDialog, keepFocus, preserveFocus, attentionCard, fmtTime: Store.util.fmtTime };
  global.HerdrShell = {
    openPalette, openDrawer, toggleTheme,
    registerCommands(list) { extraCommands.push(...list); },
  };

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
}(window));
