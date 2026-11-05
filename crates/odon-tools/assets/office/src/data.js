// The only data sources are the ODON daemon endpoints exposed through the
// local serve.mjs proxy. Nothing here invents departments or agent names.

export const MODE_ROOMS = {
  general: 'Lobby',
  code: 'Code room',
  create: 'Create room',
  documents: 'Documents room',
  files: 'Files room',
  agents: 'Delegation room',
  automations: 'Automation room',
};

export const STATE_LABELS = {
  idle: 'idle',
  thinking: 'thinking',
  calling_tool: 'calling tool',
  executing_tool: 'running tool',
  awaiting_approval: 'needs approval',
  generating_artifact: 'writing artifact',
  completed: 'completed',
  failed: 'failed',
  paused: 'paused',
  sleeping: 'sleeping',
  off_shift: 'off shift',
};

const STATE_COLORS = {
  idle: '#b7a68c',
  thinking: '#557a95',
  calling_tool: '#557a95',
  executing_tool: '#7495ad',
  awaiting_approval: '#a8775f',
  generating_artifact: '#047857',
  completed: '#047857',
  failed: '#8d5f49',
  paused: '#b45309',
  sleeping: '#8b93a1',
  off_shift: '#b9b4ab',
};

export function stateColor(state) {
  return STATE_COLORS[state] || STATE_COLORS.idle;
}

export function stateFrom(raw) {
  return raw in STATE_LABELS ? raw : 'idle';
}

export function roomFor(mode) {
  return MODE_ROOMS[mode] || MODE_ROOMS.general;
}

export function sessionIdOf(event) {
  return event.session_id || event.payload?.session_id || '';
}

export async function api(path) {
  const response = await fetch(path, { cache: 'no-store' });
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    throw new Error(data.error || `HTTP ${response.status}`);
  }
  return data;
}

export async function fetchSessions() {
  const data = await api('/api/sessions?include_subagents=true');
  return Array.isArray(data) ? data : [];
}

export function subscribe(onEvent, onStatus) {
  const source = new EventSource('/api/events');
  source.onopen = () => onStatus?.(true);
  source.onerror = () => onStatus?.(false);
  source.onmessage = (message) => {
    try {
      onEvent(JSON.parse(message.data));
    } catch {
      // A malformed frame must not take the office down.
    }
  };
  return () => source.close();
}
