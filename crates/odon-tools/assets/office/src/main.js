import {
  STATE_LABELS,
  fetchSessions,
  roomFor,
  sessionIdOf,
  stateColor,
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
  office = createOffice({ canvas, onSelect: (id) => selectDesk(id) });
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
      artifacts: [],
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
  const current = events.get(id) || { state: 'idle', tool: '', approval: null, artifacts: [] };

  switch (event.type) {
    case 'state_changed':
      current.state = event.payload.state;
      if (current.state !== 'executing_tool') current.tool = '';
      if (current.state !== 'awaiting_approval') current.approval = null;
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
      current.state = 'awaiting_approval';
      break;
    case 'approval_resolved':
      current.approval = null;
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
      artifacts: [],
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

function detailRow(label, value) {
  const dt = document.createElement('dt');
  dt.textContent = label;
  const dd = document.createElement('dd');
  dd.textContent = value;
  return [dt, dd];
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

function renderPanel(list) {
  const active = list.filter((desk) => ACTIVE_STATES.has(desk.state)).length;
  const approvals = list.filter((desk) => desk.approval).length;
  countsEl.textContent = `${list.length} desk${list.length === 1 ? '' : 's'} · ${active} active${
    approvals ? ` · ${approvals} approval` : ''
  }`;

  roomsEl.replaceChildren();
  if (list.length === 0) {
    const empty = document.createElement('p');
    empty.className = 'muted';
    empty.textContent = 'No sessions updated in the last 24 hours.';
    roomsEl.append(empty);
    renderDetail(list);
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
      row.addEventListener('click', () => selectDesk(desk.id));
      section.append(row);
    }
    roomsEl.append(section);
  }

  renderDetail(list);
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

connect();
