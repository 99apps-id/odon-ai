import {
  STATE_LABELS,
  fetchSessions,
  roomFor,
  sessionIdOf,
  stateColor,
  stateFrom,
  subscribe,
} from './data.js';
import { createOffice } from './office.js';

const canvas = document.getElementById('office');
const statusEl = document.getElementById('status');
const countsEl = document.getElementById('counts');
const roomsEl = document.getElementById('rooms');
const detailEl = document.getElementById('detail');
const configEl = document.getElementById('config');
const configDetailEl = document.getElementById('config-detail');
const retryButton = document.getElementById('retry');

const ACTIVE_STATES = new Set([
  'thinking',
  'calling_tool',
  'executing_tool',
  'awaiting_approval',
  'generating_artifact',
]);

const events = new Map();
let sessions = [];
let selectedId = '';
let office = null;
let rendererFailed = '';
let unsubscribe = null;
let pollTimer = null;
let streamOpen = false;
let reachable = false;
let renderQueued = false;

try {
  office = createOffice({
    canvas,
    onSelect: (id) => selectDesk(id),
    onHover,
  });
} catch (error) {
  rendererFailed = error.message || 'WebGL is not available.';
  showConfig(`The 3D renderer could not start: ${rendererFailed}`);
}

function showConfig(detail) {
  configDetailEl.textContent = detail;
  configEl.classList.remove('hidden');
}

function updateStatusPill() {
  statusEl.textContent = reachable ? (streamOpen ? 'live' : 'polling') : 'offline';
  statusEl.className = `pill ${reachable ? (streamOpen ? 'live' : '') : 'offline'}`;
}

function setReachable(online, detail) {
  reachable = online;
  updateStatusPill();
  if (!online) {
    showConfig(detail || 'The ODON daemon is not reachable.');
  } else if (!rendererFailed) {
    configEl.classList.add('hidden');
  }
}

function applyEvent(event) {
  if (event.type === 'subagent_progress') {
    const childId = event.payload?.subagent_session_id;
    if (!childId) return;
    const current = events.get(childId) || {
      state: 'thinking',
      tool: '',
      approval: null,
      approvalId: '',
      artifacts: [],
      thought: '',
      answer: '',
    };
    const phase = event.payload.phase;
    current.state =
      phase === 'working'
        ? 'executing_tool'
        : phase === 'finished'
          ? 'completed'
          : phase === 'failed'
            ? 'failed'
            : 'thinking';
    if (phase === 'working' && event.payload.detail) current.tool = event.payload.detail;
    events.set(childId, current);
    scheduleRender();
    return;
  }

  const id = sessionIdOf(event);
  if (!id) return;
  const current = events.get(id) || {
    state: 'idle',
    tool: '',
    approval: null,
    approvalId: '',
    artifacts: [],
    thought: '',
    answer: '',
  };

  const MAX_SNIPPET_CHARS = 600;
  const appendTail = (existing, delta) => {
    const next = existing + delta;
    return next.length > MAX_SNIPPET_CHARS
      ? next.slice(next.length - MAX_SNIPPET_CHARS)
      : next;
  };

  switch (event.type) {
    case 'thought_delta':
      current.thought = appendTail(current.thought || '', event.payload.delta);
      events.set(id, current);
      return;
    case 'text_delta':
      current.answer = appendTail(current.answer || '', event.payload.delta);
      events.set(id, current);
      return;
    case 'state_changed':
      current.state = stateFrom(event.payload.state);
      if (current.state !== 'executing_tool') current.tool = '';
      if (current.state !== 'awaiting_approval') {
        current.approval = null;
        current.approvalId = '';
      }
      break;
    case 'tool_call_started':
      current.tool = event.payload.tool_name || '';
      current.state = 'executing_tool';
      break;
    case 'tool_call_finished':
      if (!event.payload.is_error) current.tool = '';
      break;
    case 'approval_requested':
      current.approval = event.payload.tool_name || 'approval';
      current.approvalId = event.payload.approval_id || '';
      current.state = 'awaiting_approval';
      break;
    case 'approval_resolved':
      current.approval = null;
      current.approvalId = '';
      break;
    case 'artifact_created':
      current.artifacts = [event.payload.artifact?.kind || 'artifact', ...current.artifacts].slice(0, 4);
      current.state = 'generating_artifact';
      break;
    case 'error':
      current.state = 'failed';
      break;
    default:
      return;
  }

  events.set(id, current);
  scheduleRender();
}

function desks() {
  const cutoff = Date.now() - 24 * 3600 * 1000;
  return sessions
    .filter((session) => {
      const updated = Date.parse(session.updated_at || '');
      return Number.isNaN(updated) || updated >= cutoff;
    })
    .sort((a, b) => Date.parse(b.updated_at || 0) - Date.parse(a.updated_at || 0))
    .slice(0, 16)
    .map((session) => ({
      id: session.id,
      title: session.title || 'Untitled session',
      mode: session.mode || 'general',
      model: session.active_model || '',
      parentId: session.metadata?.parent_session || null,
      state: 'idle',
      tool: '',
      approval: null,
      approvalId: '',
      artifacts: [],
      thought: '',
      answer: '',
      ...(events.get(session.id) || {}),
    }));
}

function scheduleRender() {
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(() => {
    renderQueued = false;
    const list = desks();
    if (office) office.sync(list);
    renderPanel(list);
  });
}

function selectDesk(id) {
  selectedId = id;
  if (office) office.select(id);
  renderPanel(desks());
}

const filterInput = document.getElementById('filter-input');
filterInput?.addEventListener('input', () => renderPanel(desks()));

function matchesFilter(desk, query) {
  if (!query) return true;
  const q = query.toLowerCase();
  return (
    desk.title.toLowerCase().includes(q) ||
    desk.id.toLowerCase().includes(q) ||
    roomFor(desk.mode).toLowerCase().includes(q) ||
    (STATE_LABELS[desk.state] || '').toLowerCase().includes(q)
  );
}

function renderDetail(list) {
  const desk = list.find((item) => item.id === selectedId);
  detailEl.replaceChildren();
  if (!desk) {
    const hint = document.createElement('p');
    hint.className = 'muted';
    hint.textContent = 'Click a desk in the office.';
    detailEl.append(hint);
    return;
  }

  const title = document.createElement('h3');
  title.textContent = desk.title;
  const listEl = document.createElement('dl');
  for (const [label, value] of [
    ['Room', roomFor(desk.mode)],
    ['State', STATE_LABELS[desk.state] || desk.state],
    ['Model', desk.model || '—'],
    ['Tool', desk.tool || '—'],
    ['Approval', desk.approval || '—'],
    ['Artifacts', desk.artifacts.length ? desk.artifacts.join(', ') : '—'],
    ['Parent', desk.parentId || '—'],
    ['Session', desk.id],
  ]) {
    if (label === 'Parent' && !desk.parentId) continue;
    listEl.append(...detailRow(label, value));
  }
  const link = document.createElement('a');
  link.href = 'http://localhost:3000';
  link.target = '_blank';
  link.rel = 'noreferrer';
  link.textContent = 'Open in ODON';
  detailEl.append(title, listEl, link);
}

function renderPanel(rawList) {
  const query = filterInput?.value || '';
  const list = rawList.filter((desk) => matchesFilter(desk, query));
  const active = list.filter((desk) => ACTIVE_STATES.has(desk.state)).length;
  const approvals = list.filter((desk) => desk.approval).length;
  countsEl.textContent = `${list.length} desk${list.length === 1 ? '' : 's'} · ${active} active${
    approvals ? ` · ${approvals} approval` : ''
  }`;

  roomsEl.replaceChildren();
  if (rawList.length === 0) {
    const empty = document.createElement('p');
    empty.className = 'muted';
    empty.textContent = 'No sessions updated in the last 24 hours.';
    roomsEl.append(empty);
    renderDetail(rawList);
    return;
  }

  const groups = new Map();
  for (const desk of list) {
    const key = roomFor(desk.mode);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(desk);
  }

  for (const [room, items] of groups) {
    const section = document.createElement('div');
    section.className = 'room';
    const heading = document.createElement('h3');
    heading.textContent = room;
    const count = document.createElement('span');
    count.textContent = `${items.length} desk${items.length === 1 ? '' : 's'}`;
    heading.append(count);
    section.append(heading);

    for (const desk of items) {
      const row = document.createElement('button');
      row.type = 'button';
      row.className = `desk-row${desk.id === selectedId ? ' selected' : ''}`;
      const dot = document.createElement('span');
      dot.className = 'dot';
      dot.style.background = stateColor(desk.state);
      const name = document.createElement('span');
      name.textContent = desk.title;
      const state = document.createElement('small');
      state.textContent = STATE_LABELS[desk.state] || desk.state;
      row.append(dot, name, state);
      row.addEventListener('click', () => {
        selectDesk(desk.id);
        office?.focus(desk.id);
      });
      section.append(row);
    }
    roomsEl.append(section);
  }

  renderDetail(rawList);
}

async function refresh() {
  try {
    sessions = await fetchSessions();
    setReachable(true);
    const list = desks();
    if (office) office.sync(list);
    renderPanel(list);
  } catch (error) {
    setReachable(false, error.message || 'Cannot reach the ODON daemon.');
  }
}

async function connect() {
  if (unsubscribe) unsubscribe();
  unsubscribe = null;
  if (pollTimer) clearInterval(pollTimer);
  pollTimer = null;

  await refresh();
  unsubscribe = subscribe(
    applyEvent,
    (open) => {
      streamOpen = open;
      updateStatusPill();
      if (open) refresh();
    },
  );
  pollTimer = setInterval(refresh, 5000);
}

retryButton.addEventListener('click', () => {
  connect();
});

// Standalone parity with the desktop office: hover tooltip, night toggle,
// snapshot, and a director card. These are wired to the same desk data as
// the desktop view; the standalone app never invents agents.
const tooltipEl = document.getElementById('tooltip');
const nightToggle = document.getElementById('night-toggle');
const snapshotButton = document.getElementById('snapshot');
const directorCard = document.getElementById('director-card');
const focusDirector = document.getElementById('focus-director');
const lightsOn = document.getElementById('lights-on');

let nightMode = false;
let hoveredId = '';
let snippetTimer = null;

function snippetTick() {
  renderTooltip(desks());
}
function startSnippetTick() {
  if (snippetTimer) clearInterval(snippetTimer);
  snippetTimer = setInterval(snippetTick, 600);
}
function stopSnippetTick() {
  if (snippetTimer) {
    clearInterval(snippetTimer);
    snippetTimer = null;
  }
}

function renderTooltip(list) {
  if (!hoveredId) return;
  if (hoveredId === 'director') {
    tooltipEl.classList.remove('hidden');
    tooltipEl.innerHTML = '';
    const title = document.createElement('strong');
    title.textContent = 'You · Director';
    const body = document.createElement('div');
    body.textContent = 'The operator desk. Focus or turn the lights on from the card.';
    tooltipEl.append(title, body);
    return;
  }
  const desk = list.find((item) => item.id === hoveredId);
  if (!desk) {
    tooltipEl.classList.add('hidden');
    return;
  }
  tooltipEl.classList.remove('hidden');
  tooltipEl.innerHTML = '';
  const heading = document.createElement('strong');
  heading.textContent = `${desk.title} · ${STATE_LABELS[desk.state] || desk.state}`;
  const meta = document.createElement('div');
  meta.textContent = `${roomFor(desk.mode)}${desk.model ? ` · ${desk.model}` : ''}`;
  tooltipEl.append(heading, meta);
  if (desk.tool || desk.approval) {
    const chips = document.createElement('div');
    chips.className = 'chips';
    if (desk.tool) {
      const chip = document.createElement('span');
      chip.textContent = desk.tool;
      chips.append(chip);
    }
    if (desk.approval) {
      const chip = document.createElement('span');
      chip.textContent = `approval: ${desk.approval}`;
      chips.append(chip);
    }
    tooltipEl.append(chips);
  }
  const thought = (desk.thought || '').trim();
  const answer = (desk.answer || '').trim();
  if (thought) {
    const wrap = document.createElement('div');
    const label = document.createElement('b');
    label.textContent = 'Reasoning';
    const text = document.createElement('p');
    text.textContent = thought.slice(-220);
    wrap.append(label, text);
    tooltipEl.append(wrap);
  }
  if (answer) {
    const wrap = document.createElement('div');
    const label = document.createElement('b');
    label.textContent = 'Reply';
    const text = document.createElement('p');
    text.textContent = answer.slice(-220);
    wrap.append(label, text);
    tooltipEl.append(wrap);
  }
}

function handleHover(id, clientX, clientY) {
  tooltipEl.style.left = `${Math.min(clientX + 16, window.innerWidth - 336)}px`;
  tooltipEl.style.top = `${Math.min(clientY + 16, window.innerHeight - 220)}px`;
  if (id !== hoveredId) {
    hoveredId = id;
    if (id) {
      startSnippetTick();
      renderTooltip(desks());
      if (id !== 'director') {
        // Keep the director card hidden until needed.
      } else {
        directorCard?.classList.remove('hidden');
      }
    } else {
      stopSnippetTick();
      tooltipEl.classList.add('hidden');
    }
  }
}

function onHover(id, clientX, clientY) {
  handleHover(id, clientX, clientY);
}

nightToggle?.addEventListener('click', () => {
  nightMode = !nightMode;
  office?.setNight(nightMode);
  nightToggle.textContent = nightMode ? '☾ Night on' : '☀ Day';
});
snapshotButton?.addEventListener('click', () => {
  const url = office?.snapshot();
  if (!url) return;
  const link = document.createElement('a');
  link.href = url;
  link.download = 'agents-office.png';
  link.click();
});
focusDirector?.addEventListener('click', () => office?.focus('director'));
lightsOn?.addEventListener('click', () => office?.setNight(true));

connect();
