/* Screen 06 — Agents & Models. Discovered agent CLIs, model catalogue with deny policy, team builder. */
(function () {
  'use strict';

  const Store = window.OrchDemoStore;
  const UI = window.HerdrUI;
  const { h, iconEl, toast } = UI;
  const $ = (sel, root = document) => root.querySelector(sel);
  const KEY = 'herdr.agents.state';

  /* ------------------------------------------------------------------ static catalogue */

  const AGENTS = [
    { id: 'claude', name: 'Claude Code', bin: 'claude', version: 'v1.0.42', path: '/usr/local/bin/claude', models: ['opus', 'sonnet', 'haiku', 'claude-fable'], skills: true, caps: ['Plan mode', 'Tool use', 'Hooks', 'Herdr detection'],
      badges: [['Logged In', 'emerald', 'checkcircle'], ['Skills Installed', 'emerald', 'check'], ['Herdr Detect: ON', 'emerald', 'check']] },
    { id: 'codex', name: 'Codex CLI', bin: 'codex', version: 'v0.9.8', path: '/usr/local/bin/codex', models: ['gpt-6-sol'], skills: true, caps: ['Sandboxed exec', 'Tool use', 'Skills'],
      badges: [['Logged In', 'emerald', 'checkcircle'], ['Skills Installed', 'emerald', 'check']] },
    { id: 'opencode', name: 'OpenCode', bin: 'opencode', version: 'v0.4.15', path: '/home/arun/.local/bin/opencode', models: ['deepseek-v4-pro'], skills: true, caps: ['Custom commands', 'Tool use', 'Open-weight models'],
      badges: [['Ready', 'emerald', 'checkcircle'], ['Commands Installed', 'emerald', 'check']] },
    { id: 'amp', name: 'Amp', bin: 'amp', version: null, path: '/usr/local/bin/amp', models: ['sonnet'], skills: false, caps: ['Tool use', 'Thread sharing'],
      badges: [['Logged In', 'emerald', 'checkcircle']] },
    { id: 'antigravity', name: 'Antigravity', bin: 'agy', version: null, path: '/usr/local/bin/agy', models: ['astra-ultra'], skills: false, caps: ['Agent manager', 'Tool use'],
      badges: [['Logged In', 'emerald', 'checkcircle'], ['Available', 'emerald', 'check']] },
    { id: 'hermes', name: 'Hermes', bin: 'hermes', version: null, path: '/usr/local/bin/hermes', models: [], skills: false, unverified: true, caps: ['Gateway env'],
      badges: [['Unverified Flags · Gateway Env', 'amber', 'warning']] },
  ];

  const MODELS = [
    { id: 'opus', tier: 'high', provider: 'Anthropic · Proprietary', score: 5, ctx: '200k' },
    { id: 'deepseek-v4-pro', tier: 'high', provider: 'DeepSeek · Open-Weight', score: 5, ctx: '128k' },
    { id: 'sonnet', tier: 'medium', provider: 'Anthropic · Proprietary', score: 4, ctx: '200k' },
    { id: 'gpt-6-sol', tier: 'medium', provider: 'OpenAI · Proprietary', score: 4, ctx: '128k' },
    { id: 'haiku', tier: 'small', provider: 'Anthropic · Proprietary', score: 3, ctx: '64k' },
    { id: 'astra-ultra', tier: 'high', provider: 'Astra · Proprietary', score: null, ctx: null },
    { id: 'claude-fable', tier: 'high', provider: 'Anthropic (Fable) · Proprietary', score: null, ctx: null },
  ];
  const ROLES = [
    ['lead', 'Lead'], ['reviewer', 'Reviewer'], ['coder-1', 'Coder-1'], ['coder-2', 'Coder-2'],
  ];
  const TIER_TONE = { small: 'neutral', medium: 'amber', high: 'coral' };

  /* ------------------------------------------------------------------ state */

  function seedRoles() {
    const d = Store.data();
    const get = (id) => d.agents.find((a) => a.id === id);
    const pick = (a, fb) => (a ? { agent: a.cli, model: a.model } : fb);
    return {
      lead: { agent: d.status.lead.agent, model: d.status.lead.model },
      reviewer: pick(get('reviewer'), { agent: 'claude', model: 'opus' }),
      'coder-1': pick(get('coder-1'), { agent: 'claude', model: 'sonnet' }),
      'coder-2': pick(get('coder-2'), { agent: 'codex', model: 'gpt-6-sol' }),
    };
  }
  function seedTiers() {
    const t = {};
    [['small', 'claude', 'haiku'], ['medium', 'claude', 'sonnet'], ['high', 'claude', 'opus']].forEach(([k, a, m]) => { t[k] = { agent: a, model: m }; });
    return t;
  }

  const state = (() => {
    const base = { roles: seedRoles(), tiers: seedTiers(), deny: ['*astra*', '*fable*'], benched: { amp: true }, synced: [], scans: 0 };
    try { return { ...base, ...JSON.parse(localStorage.getItem(KEY) || '{}') }; } catch (_) { return base; }
  })();
  const save = () => { try { localStorage.setItem(KEY, JSON.stringify(state)); } catch (_) { /* storage unavailable */ } };

  let tab = 'installed';
  let scanning = false;
  let draft = null;
  let filters = { q: '', tier: 'all', status: 'all' };
  let detailId = null;

  Store.api.get('/api/v1/models').then((m) => {
    if (localStorage.getItem(KEY)) return;
    m.tiers.forEach((t) => { state.tiers[t.tier] = { agent: t.agent, model: t.model }; });
    render();
  }).catch(() => {});

  /* ------------------------------------------------------------------ helpers */

  const badgeCls = (tone) => (tone === 'neutral' ? 'badge' : `badge badge--${tone}`);
  const agentById = (id) => AGENTS.find((a) => a.id === id);
  const modelById = (id) => MODELS.find((m) => m.id === id);
  const benched = (a) => !!state.benched[a.id];

  function globRe(p) {
    return new RegExp(`^${p.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*').replace(/\?/g, '.')}$`, 'i');
  }
  function denyMatch(modelId) {
    return state.deny.find((p) => globRe(p).test(modelId)) || null;
  }
  const allowed = (modelId) => !denyMatch(modelId);

  function options() {
    const out = [];
    AGENTS.forEach((a) => { if (!benched(a) && !a.unverified) a.models.forEach((m) => { if (allowed(m)) out.push({ agent: a.id, model: m }); }); });
    return out;
  }

  function roleChips(agentId) {
    const groups = {};
    ROLES.forEach(([key, label]) => {
      const r = state.roles[key];
      if (r && r.agent === agentId) (groups[r.model] = groups[r.model] || []).push(label);
    });
    return Object.entries(groups).map(([model, labels]) => `${labels.join(' & ')} (${model})`);
  }

  async function copyText(text, what) {
    try { await navigator.clipboard.writeText(text); toast(`Copied ${what || text}`, 'cyan', 'copy'); }
    catch (_) { toast(`Copy is blocked in this context. Value: ${text}`, 'amber', 'warning'); }
  }

  function svgEl(inner, color) {
    const t = document.createElement('template');
    t.innerHTML = `<svg viewBox="0 0 40 40" fill="none" stroke="${color}" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">${inner}</svg>`;
    return t.content.firstElementChild;
  }
  function glyph(id) {
    switch (id) {
      case 'claude': return svgEl('<path d="M20 5v30M5 20h30M9.4 9.4l21.2 21.2M30.6 9.4L9.4 30.6"/>', '#D97757');
      case 'codex': return svgEl('<path d="M20 5l12 7v14l-12 7-12-7V12z"/><path d="M20 13l6 3.5v7L20 27l-6-3.5v-7z"/>', 'var(--text-primary)');
      case 'opencode': return svgEl('<path d="M14 12L6 20l8 8M26 12l8 8-8 8M23 10l-6 20"/>', 'var(--accent-cyan)');
      case 'amp': return svgEl('<path d="M23 4L9 22h10l-2 14 14-18H21z" fill="currentColor"/>', 'var(--accent-violet)');
      case 'antigravity': return svgEl('<path d="M20 6L6 33h8l6-12 6 12h8z"/>', 'var(--accent-cyan)');
      default: return svgEl('<circle cx="15" cy="20" r="8"/><circle cx="25" cy="20" r="8"/>', 'var(--accent-amber)');
    }
  }

  /* ------------------------------------------------------------------ agent cards */

  function agentBadges(a) {
    const list = a.badges.map(([t, tone, ic]) => [t, tone, ic]);
    if (state.synced.includes(a.id) && !a.badges.some((b) => /Skills|Commands/.test(b[0]))) list.push(['Skills Installed', 'emerald', 'check']);
    if (a.id === 'amp' && benched(a)) list.push(['Benched (0 sessions)', 'neutral', 'pause']);
    if (a.id === 'amp' && !benched(a)) list.push(['Active', 'emerald', 'check']);
    return list;
  }

  function agentCard(a) {
    const roles = roleChips(a.id);
    const isLead = state.roles.lead.agent === a.id;
    return h('button', {
      type: 'button', class: `card acard${isLead ? ' card--tone tone-emerald' : ''}${benched(a) ? ' acard--benched' : ''}`, 'data-fk': `agent-${a.id}`,
      'aria-haspopup': 'dialog', 'aria-label': `${a.name} details`, on: { click: () => openDetail(a.id) },
    },
      h('span', { class: 'acard__logo' }, glyph(a.id)),
      h('span', { class: 'acard__main' },
        h('span', { class: 'acard__top' },
          h('span', { class: 'acard__name' }, a.name, ' ', h('span', { class: 'mono muted' }, `(${a.bin})`)),
          a.version ? h('span', { class: 'badge badge--mono' }, a.version) : null),
        h('span', { class: 'acard__badges' }, agentBadges(a).map(([t, tone, ic]) => h('span', { class: badgeCls(tone) }, iconEl(ic, 12), t))),
        h('span', { class: 'acard__role' },
          iconEl('user', 16), h('span', { class: 'muted' }, 'Assigned Role:'),
          roles.length ? roles.map((r) => h('span', { class: 'badge badge--violet badge--mono' }, r)) : h('span', { class: 'muted' }, '—'))));
  }

  function agentsSection() {
    return h('section', { class: 'card sect', 'aria-labelledby': 'ag-title' },
      h('header', { class: 'card__head sect__head' },
        h('h2', { class: 'card__title', id: 'ag-title' }, iconEl('cube'), `Discovered Agent CLIs (${AGENTS.length})`),
        h('span', { class: 'sect__note', role: 'status' }, h('span', { class: 'dot dot--emerald', 'aria-hidden': 'true' }), `${AGENTS.length} agents • CLI discovery, auth & capability status`)),
      h('div', { class: 'agrid' }, AGENTS.map(agentCard)));
  }

  /* ------------------------------------------------------------------ catalogue */

  function policyCell(m) {
    const pat = denyMatch(m.id);
    return h('button', {
      type: 'button', class: `pol ${pat ? 'pol--deny' : 'pol--ok'}`, 'data-fk': `pol-${m.id}`,
      'aria-label': pat ? `${m.id}: denied by policy ${pat}. Activate to review.` : `${m.id}: allowed. Activate to deny.`,
      on: { click: () => togglePolicy(m) },
    }, iconEl(pat ? 'stop' : 'check', 14), pat ? `DENIED BY POLICY (${pat})` : 'Allowed');
  }

  function togglePolicy(m) {
    const pat = denyMatch(m.id);
    if (!pat) {
      const used = ROLES.filter(([k]) => state.roles[k].model === m.id).map(([, l]) => l);
      if (used.length) { toast(`${m.id} is assigned to ${used.join(', ')}. Reassign it in Team Builder first.`, 'amber', 'warning'); return; }
      state.deny.push(m.id); save(); render();
      toast(`${m.id} is now denied by policy`, 'coral', 'shield');
    } else if (pat === m.id) {
      state.deny = state.deny.filter((p) => p !== pat); save(); render();
      toast(`${m.id} is allowed again`, 'emerald', 'check');
    } else {
      setTab('catalog');
      toast(`${m.id} matches the pattern ${pat}. Remove the pattern below to allow it.`, 'amber', 'shield');
    }
  }

  function dots(score) {
    return h('span', { class: 'sdots', 'aria-hidden': 'true' }, [1, 2, 3, 4, 5].map((i) => h('i', { class: i <= score ? 'on' : null })));
  }

  function catalogRows() {
    const q = filters.q.trim().toLowerCase();
    return MODELS.filter((m) => (!q || m.id.includes(q) || m.provider.toLowerCase().includes(q))
      && (filters.tier === 'all' || m.tier === filters.tier)
      && (filters.status === 'all' || (filters.status === 'allowed') === allowed(m.id)));
  }

  function catalogSection(withTools) {
    const rows = withTools ? catalogRows() : MODELS;
    const body = rows.map((m) => {
      const ok = allowed(m.id);
      return h('tr', { class: ok ? null : 'is-denied' },
        h('td', { class: 'mono muted' }, String(MODELS.indexOf(m) + 1).padStart(3, '0')),
        h('td', null, h('span', { class: `mchip mono${ok ? '' : ' mchip--deny'}` }, m.id)),
        h('td', null, h('span', { class: badgeCls(TIER_TONE[m.tier]) }, m.tier)),
        h('td', null, m.provider),
        h('td', null, m.score ? h('span', { class: 'score' }, h('span', { class: 'mono' }, `${m.score} / 5`), dots(m.score)) : h('span', { class: 'muted' }, '–')),
        h('td', { class: 'mono' }, m.ctx || '–'),
        h('td', null, policyCell(m)));
    });

    const tools = withTools ? h('div', { class: 'tools' },
      h('div', { class: 'field tools__q' }, h('label', { for: 'f-q' }, 'Search models'),
        h('input', { class: 'input', id: 'f-q', type: 'search', value: filters.q, placeholder: 'Model id or provider', 'data-fk': 'f-q', on: { input: (e) => { const pos = e.target.selectionStart; filters.q = e.target.value; render(); const n = document.querySelector('[data-fk="f-q"]'); if (n) n.setSelectionRange(pos, pos); } } })),
      h('div', { class: 'field' }, h('label', { for: 'f-tier' }, 'Tier'),
        h('select', { class: 'select', id: 'f-tier', 'data-fk': 'f-tier', on: { change: (e) => { filters.tier = e.target.value; render(); } } },
          ['all', 'small', 'medium', 'high'].map((v) => h('option', { value: v, selected: filters.tier === v || null }, v === 'all' ? 'All tiers' : v)))),
      h('div', { class: 'field' }, h('label', { for: 'f-status' }, 'Policy'),
        h('select', { class: 'select', id: 'f-status', 'data-fk': 'f-status', on: { change: (e) => { filters.status = e.target.value; render(); } } },
          [['all', 'All'], ['allowed', 'Allowed'], ['denied', 'Denied']].map(([v, l]) => h('option', { value: v, selected: filters.status === v || null }, l))))) : null;

    return h('section', { class: 'card sect', 'aria-labelledby': 'cat-title' },
      h('header', { class: 'card__head sect__head' },
        h('h2', { class: 'card__title', id: 'cat-title' }, iconEl('layers'), 'Shipped Model Catalog & Deny Policy'),
        h('span', { class: 'sect__note' }, iconEl('shield', 14), `Models: ${MODELS.length} • Policy: allow / deny (pattern match)`)),
      tools,
      h('div', { class: 'cat__scroll' },
        h('table', { class: 'table cat__table' },
          h('thead', null, h('tr', null, ['#', 'Model ID', 'Tier', 'Provider Kinds', 'Reasoning / Coding Score', 'Context', 'Policy Status'].map((c) => h('th', { scope: 'col' }, c)))),
          h('tbody', null, rows.length ? body : h('tr', null, h('td', { colspan: '7', class: 'cat__empty' }, 'No models match these filters.'))))));
  }

  function policyEditor() {
    const input = h('input', { class: 'input mono', id: 'pat-input', placeholder: '*preview*', autocomplete: 'off', 'data-fk': 'pat-input', 'aria-describedby': 'pat-err pat-hint' });
    const err = h('p', { class: 'form-err text-coral', id: 'pat-err', role: 'alert' });
    const add = () => {
      const v = input.value.trim().toLowerCase();
      if (!/^[a-z0-9*?._-]+$/.test(v)) { err.textContent = 'Use letters, numbers, dots, dashes and * or ? wildcards.'; input.focus(); return; }
      if (state.deny.includes(v)) { err.textContent = 'That pattern is already in the policy.'; input.focus(); return; }
      const hit = ROLES.filter(([k]) => globRe(v).test(state.roles[k].model)).map(([, l]) => l);
      if (hit.length) { err.textContent = `Pattern would deny a model assigned to ${hit.join(', ')}. Reassign it first.`; input.focus(); return; }
      state.deny.push(v); save(); toast(`Pattern ${v} added to deny policy`, 'coral', 'shield'); render();
      const n = document.querySelector('[data-fk="pat-input"]'); if (n) n.focus();
    };
    input.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); add(); } });
    return h('section', { class: 'card sect', 'aria-labelledby': 'pol-title' },
      h('header', { class: 'card__head' }, h('h2', { class: 'card__title', id: 'pol-title' }, iconEl('shield'), 'Deny Policy Patterns')),
      h('div', { class: 'pol-ed' },
        h('p', { class: 'muted', id: 'pat-hint' }, 'A model whose id matches any pattern is never offered to the lead or assigned to a role. ', h('span', { class: 'mono' }, '*'), ' matches any run of characters.'),
        h('ul', { class: 'pats', 'aria-label': 'Deny patterns' }, state.deny.length
          ? state.deny.map((p) => h('li', { class: 'pat mono' }, p, h('button', { type: 'button', class: 'iconbtn', 'data-fk': `rm-${p}`, 'aria-label': `Remove pattern ${p}`, on: { click: () => { state.deny = state.deny.filter((x) => x !== p); save(); toast(`Pattern ${p} removed`, 'cyan', 'check'); render(); } } }, iconEl('x', 12))))
          : h('li', { class: 'muted' }, 'No patterns. Every catalogue model is allowed.')),
        h('div', { class: 'pol-ed__add' },
          h('div', { class: 'field pol-ed__field' }, h('label', { for: 'pat-input' }, 'Add pattern'), input),
          h('button', { type: 'button', class: 'btn btn--coral', on: { click: add } }, iconEl('plus', 14), 'Add to deny list')),
        err));
  }

  /* ------------------------------------------------------------------ team builder */

  function comboValue(c) { return `${c.agent}:${c.model}`; }

  function builder() {
    if (!draft) draft = JSON.parse(JSON.stringify({ roles: state.roles, tiers: state.tiers }));
    const opts = options();
    const select = (id, label, cur, onChange) => {
      const list = opts.some((o) => comboValue(o) === comboValue(cur)) ? opts : [cur, ...opts];
      return h('div', { class: 'field' }, h('label', { for: id }, label),
        h('select', { class: 'select mono', id, 'data-fk': id, on: { change: (e) => { const [agent, model] = e.target.value.split(':'); onChange({ agent, model }); render(); } } },
          list.map((o) => h('option', { value: comboValue(o), selected: comboValue(o) === comboValue(cur) || null }, `${agentById(o.agent).name} · ${o.model}${opts.includes(o) ? '' : ' (unavailable)'}`))));
    };
    const dirty = JSON.stringify(draft) !== JSON.stringify({ roles: state.roles, tiers: state.tiers });
    const cmd = ROLES.map(([k]) => `${k}=${draft.roles[k].agent}:${draft.roles[k].model}`).join(' ');
    const stale = ROLES.filter(([k]) => !opts.some((o) => comboValue(o) === comboValue(draft.roles[k]))).map(([, l]) => l);

    return h('div', { class: 'tb' },
      h('section', { class: 'card sect', 'aria-labelledby': 'tb-roles' },
        h('header', { class: 'card__head' }, h('h2', { class: 'card__title', id: 'tb-roles' }, iconEl('users'), 'Role Assignments')),
        h('div', { class: 'tb__grid' }, ROLES.map(([k, l]) => select(`role-${k}`, l, draft.roles[k], (v) => { draft.roles[k] = v; })))),
      h('section', { class: 'card sect', 'aria-labelledby': 'tb-tiers' },
        h('header', { class: 'card__head' }, h('h2', { class: 'card__title', id: 'tb-tiers' }, iconEl('layers'), 'Tier Routing')),
        h('div', { class: 'tb__grid' }, ['small', 'medium', 'high'].map((t) => select(`tier-${t}`, `Milestones tagged ${t}`, draft.tiers[t], (v) => { draft.tiers[t] = v; })))),
      h('section', { class: 'card sect' },
        h('div', { class: 'tb__foot' },
          h('div', { class: 'tb__cmd' }, h('span', { class: 'muted' }, 'Equivalent CLI'), h('code', { class: 'mono tb__code' }, `orch agents assign ${cmd}`)),
          stale.length ? h('p', { class: 'text-amber tb__warn', role: 'status' }, iconEl('warning', 14), `${stale.join(', ')} uses a model that is denied, benched or unverified.`) : null,
          h('div', { class: 'tb__btns' },
            h('button', { type: 'button', class: 'btn', 'data-fk': 'tb-copy', on: { click: () => copyText(`orch agents assign ${cmd}`, 'assign command') } }, iconEl('copy', 14), 'Copy command'),
            h('button', { type: 'button', class: 'btn', disabled: !dirty || null, 'data-fk': 'tb-reset', on: { click: () => { draft = null; render(); toast('Changes discarded', 'cyan', 'x'); } } }, 'Discard'),
            h('button', { type: 'button', class: 'btn btn--emerald btn--solid', disabled: !dirty || stale.length > 0 || null, 'data-fk': 'tb-apply', on: { click: applyTeam } }, iconEl('check', 14), 'Apply team')))));
  }

  function applyTeam() {
    state.roles = draft.roles; state.tiers = draft.tiers; draft = null; save();
    toast('Team applied. Role assignments updated.', 'emerald', 'check');
    render();
  }

  /* ------------------------------------------------------------------ dialogs */

  const detail = h('dialog', { class: 'modal', id: 'agent-dialog', closedby: 'any', 'aria-labelledby': 'agent-title' });
  const doctor = h('dialog', { class: 'modal', id: 'doctor-dialog', closedby: 'any', 'aria-labelledby': 'doctor-title' });
  document.body.append(detail, doctor);
  [detail, doctor].forEach(UI.enhanceDialog);
  detail.addEventListener('close', () => { const id = detailId; detailId = null; const b = id && document.querySelector(`[data-fk="agent-${id}"]`); if (b) b.focus(); });

  function openDetail(id) { detailId = id; fillDetail(); if (!detail.open) detail.showModal(); }

  function fillDetail() {
    const a = agentById(detailId);
    if (!a) return;
    const roles = roleChips(a.id);
    detail.replaceChildren(
      h('div', { class: 'modal__head' },
        h('h2', { class: 'modal__title dlg__title', id: 'agent-title' }, h('span', { class: 'dlg__logo' }, glyph(a.id)), a.name),
        h('button', { type: 'button', class: 'btn btn--ghost btn--icon btn--sm', 'aria-label': 'Close', on: { click: () => detail.close() } }, iconEl('x'))),
      h('div', { class: 'modal__body' },
        h('dl', { class: 'dl' },
          h('dt', null, 'Binary'), h('dd', { class: 'mono' }, a.path),
          h('dt', null, 'Version'), h('dd', { class: 'mono' }, a.version || 'not reported'),
          h('dt', null, 'Status'), h('dd', null, h('span', { class: 'acard__badges' }, agentBadges(a).map(([t, tone, ic]) => h('span', { class: badgeCls(tone) }, iconEl(ic, 12), t)))),
          h('dt', null, 'Capabilities'), h('dd', null, h('span', { class: 'acard__badges' }, a.caps.map((c) => h('span', { class: 'badge' }, c)))),
          h('dt', null, 'Models'), h('dd', null, a.models.length
            ? h('span', { class: 'acard__badges' }, a.models.map((m) => h('span', { class: `mchip mono${allowed(m) ? '' : ' mchip--deny'}`, title: allowed(m) ? 'Allowed' : `Denied by ${denyMatch(m)}` }, m)))
            : h('span', { class: 'muted' }, 'None verified')),
          h('dt', null, 'Roles'), h('dd', null, roles.length ? roles.map((r) => h('span', { class: 'badge badge--violet badge--mono' }, r)) : h('span', { class: 'muted' }, 'Unassigned'))),
        a.unverified ? h('p', { class: 'text-amber' }, 'Launch flags for this agent could not be verified, so it cannot take a role until Run Doctor passes.') : null),
      h('div', { class: 'modal__foot' },
        h('button', { type: 'button', class: 'btn', on: { click: () => { detail.close(); setTab('team'); } } }, iconEl('users', 14), 'Assign role'),
        h('button', {
          type: 'button', class: `btn ${benched(a) ? 'btn--emerald' : 'btn--amber'}`, 'data-fk': 'dlg-bench',
          on: { click: () => benchToggle(a) },
        }, iconEl(benched(a) ? 'play' : 'pause', 14), benched(a) ? 'Return to pool' : 'Bench agent'),
        h('button', { type: 'button', class: 'btn btn--cyan', on: { click: () => { detail.close(); toast(`${a.name} re-checked: ${a.unverified ? 'flags still unverified' : 'auth and capabilities OK'}`, a.unverified ? 'amber' : 'emerald', a.unverified ? 'warning' : 'checkcircle'); } } }, iconEl('sync', 14), 'Re-check')));
  }

  function benchToggle(a) {
    const used = ROLES.filter(([k]) => state.roles[k].agent === a.id).map(([, l]) => l);
    if (!benched(a) && used.length) { toast(`${a.name} holds ${used.join(', ')}. Reassign those roles before benching it.`, 'amber', 'warning'); return; }
    state.benched[a.id] = !benched(a); save();
    toast(`${a.name} ${benched(a) ? 'benched' : 'returned to the pool'}`, benched(a) ? 'amber' : 'emerald', benched(a) ? 'pause' : 'play');
    render(); fillDetail();
  }

  function doctorLines() {
    const hd = Store.data().status.herdr;
    const L = [];
    L.push([hd.connected, `herdr ${hd.version} reachable at ${hd.socket}`]);
    L.push([true, `driver: ${Store.data().status.driver.command} (pid ${Store.data().status.driver.pid})`]);
    AGENTS.forEach((a) => {
      if (a.unverified) L.push([false, `${a.bin}: launch flags unverified (gateway env not detected)`]);
      else if (benched(a)) L.push([true, `${a.bin}: benched, skipped`]);
      else L.push([true, `${a.bin}: ${a.version || 'version n/a'}, auth ok`]);
    });
    ROLES.forEach(([k]) => { const r = state.roles[k]; L.push([allowed(r.model), `role ${k}: ${r.agent}:${r.model}${allowed(r.model) ? '' : ' denied by policy'}`]); });
    L.push([true, `deny policy: ${state.deny.length ? state.deny.join(' ') : 'empty'}`]);
    return L;
  }

  function runDoctor() {
    const lines = doctorLines();
    const bad = lines.filter((l) => !l[0]).length;
    const text = lines.map(([ok, t]) => `${ok ? 'ok  ' : 'warn'} ${t}`).join('\n');
    doctor.replaceChildren(
      h('div', { class: 'modal__head' }, h('h2', { class: 'modal__title', id: 'doctor-title' }, 'orch doctor'), h('span', { class: badgeCls(bad ? 'amber' : 'emerald') }, bad ? `${bad} warning${bad === 1 ? '' : 's'}` : 'All checks passed')),
      h('div', { class: 'modal__body' },
        h('pre', { class: 'code doc__out', tabindex: '0', 'aria-label': 'Doctor report' }, lines.map(([ok, t]) => h('span', { class: `doc__ln ${ok ? 'text-emerald' : 'text-amber'}` }, h('b', null, ok ? 'ok  ' : 'warn'), ` ${t}\n`)))),
      h('div', { class: 'modal__foot' },
        h('button', { type: 'button', class: 'btn', on: { click: () => copyText(text, 'doctor report') } }, iconEl('copy', 14), 'Copy report'),
        h('button', { type: 'button', class: 'btn btn--solid btn--cyan', autofocus: true, on: { click: () => doctor.close() } }, 'Close')));
    doctor.showModal();
  }

  /* ------------------------------------------------------------------ toolbar actions */

  function scan() {
    if (scanning) return;
    scanning = true; render();
    setTimeout(() => {
      scanning = false; state.scans += 1; save(); render();
      toast(`Scanned ${AGENTS.length} agents. ${AGENTS.filter((a) => !a.unverified).length} verified, 1 needs attention.`, 'cyan', 'sync');
    }, 700);
  }

  function syncSkills() {
    const todo = AGENTS.filter((a) => !a.unverified && !a.badges.some((b) => /Skills|Commands/.test(b[0])) && !state.synced.includes(a.id));
    if (!todo.length) { toast('Skills are already in sync for every verified agent.', 'cyan', 'check'); return; }
    todo.forEach((a) => state.synced.push(a.id)); save(); render();
    toast(`orch agents sync: skills installed for ${todo.map((a) => a.bin).join(', ')}`, 'emerald', 'sync');
  }

  /* ------------------------------------------------------------------ tabs + render */

  const TABS = [['installed', 'cube', () => `Installed Agents (${AGENTS.length})`], ['catalog', 'layers', () => `Model Catalog & Policy (${MODELS.length})`], ['team', 'users', () => 'Team Builder']];

  function setTab(id) {
    tab = id; render();
    const t = document.getElementById(`tab-${id}`); if (t) t.focus();
  }

  function tabKeys(e) {
    const ids = TABS.map((t) => t[0]);
    let i = ids.indexOf(tab);
    if (e.key === 'ArrowRight') i = (i + 1) % ids.length;
    else if (e.key === 'ArrowLeft') i = (i - 1 + ids.length) % ids.length;
    else if (e.key === 'Home') i = 0;
    else if (e.key === 'End') i = ids.length - 1;
    else return;
    e.preventDefault();
    setTab(ids[i]);
  }

  function render() {
    UI.preserveFocus(() => {
      const panel = tab === 'installed' ? [agentsSection(), catalogSection(false)]
        : tab === 'catalog' ? [catalogSection(true), policyEditor()]
          : [builder()];
      $('#page').replaceChildren(
        h('header', { class: 'pg__head' },
          h('span', { class: 'pg__icon' }, iconEl('robot')),
          h('div', null, h('h1', { class: 'pg__title' }, 'Agents & Models'), h('p', { class: 'pg__sub' }, 'Discover, manage and assign agent CLIs, models and role policies for orchestration.'))),
        h('div', { class: 'bar2' },
          h('div', { class: 'tabs', role: 'tablist', 'aria-label': 'Agents and models', on: { keydown: tabKeys } },
            TABS.map(([id, icon, label]) => h('button', {
              type: 'button', class: 'tabbtn', role: 'tab', id: `tab-${id}`, 'aria-selected': String(tab === id), 'aria-controls': 'tabpanel', tabindex: tab === id ? '0' : '-1',
              on: { click: () => { tab = id; render(); } },
            }, iconEl(icon, 16), label()))),
          h('div', { class: 'bar2__actions' },
            h('button', { type: 'button', class: 'btn', id: 'scan-btn', disabled: scanning || null, 'aria-busy': String(scanning), 'data-fk': 'scan', on: { click: scan } },
              (() => { const i = iconEl('sync', 16); if (scanning) i.classList.add('spin'); return i; })(), scanning ? 'Scanning…' : 'Scan Agents'),
            h('button', { type: 'button', class: 'btn', id: 'sync-btn', 'data-fk': 'sync', on: { click: syncSkills } }, iconEl('doc', 16), 'Sync Skills (orch agents sync)'),
            h('button', { type: 'button', class: 'btn', id: 'doctor-btn', 'data-fk': 'doctor', 'aria-haspopup': 'dialog', on: { click: runDoctor } }, iconEl('shield', 16), 'Run Doctor'))),
        h('div', { class: 'panel', role: 'tabpanel', id: 'tabpanel', 'aria-labelledby': `tab-${tab}` }, panel));
    });
  }

  window.HerdrShell.registerCommands([
    { group: 'Agents', label: 'Scan agents', icon: 'sync', hint: 'discover', run: scan },
    { group: 'Agents', label: 'Run doctor', icon: 'shield', hint: 'orch doctor', run: runDoctor },
    { group: 'Agents', label: 'Sync skills', icon: 'doc', hint: 'orch agents sync', run: syncSkills },
    { group: 'Agents', label: 'Open model catalog & policy', icon: 'layers', hint: 'models', run: () => setTab('catalog') },
    { group: 'Agents', label: 'Open team builder', icon: 'users', hint: 'roles', run: () => setTab('team') },
  ]);

  Store.subscribe((reason) => { if (reason === 'project' && !localStorage.getItem(KEY)) { state.roles = seedRoles(); draft = null; render(); } });
  render();
}());
