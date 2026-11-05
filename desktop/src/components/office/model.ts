import type { Agent, AgentEvent, Session } from '../../types';

export type DeskState =
  | 'idle'
  | 'thinking'
  | 'calling_tool'
  | 'executing_tool'
  | 'awaiting_approval'
  | 'generating_artifact'
  | 'completed'
  | 'failed'
  | 'paused'
  | 'sleeping'
  | 'off_shift';

export interface Desk {
  id: string;
  title: string;
  mode: string;
  model: string;
  parentId: string | null;
  state: DeskState;
  tool: string;
  approval: string;
  /** Live approval id, so the office can resolve it without opening chat. */
  approvalId?: string;
  artifacts: string[];
  updatedAt: string;
  /** Present when the desk is a persisted agent rather than a bare session. */
  agentId?: string;
  role?: string;
  color?: string;
  /** The agent's most recent conversation, when it has one. */
  sessionId?: string;
  onShift?: boolean;
  workHours?: string;
  /** Active subagent delegations under this desk's current conversation. */
  children?: number;
}

export interface DeskEventState {
  state: DeskState;
  tool: string;
  approval: string;
  approvalId: string;
  artifacts: string[];
  /** Tail of the latest reasoning stream, for the hover tooltip. */
  thought: string;
  /** Tail of the latest answer stream, for the hover tooltip. */
  answer: string;
}

export const MODE_ROOMS: Record<string, string> = {
  general: 'Lobby',
  code: 'Code room',
  create: 'Create room',
  documents: 'Documents room',
  files: 'Files room',
  agents: 'Delegation room',
  automations: 'Automation room',
};

export const ROOM_ORDER = [
  'Lobby',
  'Code room',
  'Create room',
  'Documents room',
  'Files room',
  'Delegation room',
  'Automation room',
];

export const AREA_AGENT = 'Agent Floor';
export const AREA_SUBAGENT = 'Subagent Wing';
export const AREA_ORDER = [AREA_AGENT, AREA_SUBAGENT];

/** Subagent sessions sit in their own wing; top-level sessions on the floor. */
export function areaFor(desk: Pick<Desk, 'parentId'>): string {
  return desk.parentId ? AREA_SUBAGENT : AREA_AGENT;
}

export const STATE_LABELS: Record<DeskState, string> = {
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

export const STATE_COLORS: Record<DeskState, string> = {
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

export const ACTIVE_STATES: ReadonlySet<DeskState> = new Set<DeskState>([
  'thinking',
  'calling_tool',
  'executing_tool',
  'awaiting_approval',
  'generating_artifact',
]);

const KNOWN_STATES = new Set<string>(Object.keys(STATE_LABELS));

export function stateFrom(raw: string): DeskState {
  return KNOWN_STATES.has(raw) ? (raw as DeskState) : 'idle';
}

export function roomFor(mode: string): string {
  return MODE_ROOMS[mode] || MODE_ROOMS.general;
}

export function emptyDeskState(): DeskEventState {
  return {
    state: 'idle',
    tool: '',
    approval: '',
    approvalId: '',
    artifacts: [],
    thought: '',
    answer: '',
  };
}

/** Keep a bounded tail so a long session cannot grow the office state. */
const MAX_SNIPPET_CHARS = 600;

function appendTail(existing: string, delta: string): string {
  const next = existing + delta;
  return next.length > MAX_SNIPPET_CHARS
    ? next.slice(next.length - MAX_SNIPPET_CHARS)
    : next;
}

/**
 * Fold one daemon event into the per-session live state.
 *
 * Text/thought deltas are ignored on purpose: the office shows what an agent
 * is doing, not its prose, and applying every token would re-render the scene
 * hundreds of times per second.
 */
export function applyOfficeEvent(
  events: Map<string, DeskEventState>,
  event: AgentEvent,
): boolean {
  if (event.type === 'subagent_progress') {
    const childId = event.payload.subagent_session_id;
    if (!childId) return false;
    const current = events.get(childId) || emptyDeskState();
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
    return true;
  }

  const id = event.session_id || event.payload.session_id || '';
  if (!id) return false;
  const current = events.get(id) || emptyDeskState();

  switch (event.type) {
    case 'thought_delta':
      // Text streams update the tooltip only; returning false keeps the scene
      // and the panel out of the per-token render path.
      current.thought = appendTail(current.thought, event.payload.delta);
      events.set(id, current);
      return false;
    case 'text_delta':
      current.answer = appendTail(current.answer, event.payload.delta);
      events.set(id, current);
      return false;
    case 'state_changed':
      current.state = stateFrom(event.payload.state);
      if (current.state !== 'executing_tool') current.tool = '';
      if (current.state !== 'awaiting_approval') {
        current.approval = '';
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
      current.approval = '';
      current.approvalId = '';
      break;
    case 'artifact_created':
      current.artifacts = [event.payload.artifact.kind, ...current.artifacts].slice(0, 4);
      current.state = 'generating_artifact';
      break;
    case 'error':
      current.state = 'failed';
      break;
    default:
      return false;
  }

  events.set(id, current);
  return true;
}

/**
 * Build the visible roster from real sessions. The list endpoint is unbounded
 * history, so the office keeps the last 24 hours and at most `limit` desks.
 */
export function desksFromSessions(
  sessions: Session[],
  events: Map<string, DeskEventState>,
  limit = 16,
  now = Date.now(),
): Desk[] {
  const cutoff = now - 24 * 60 * 60 * 1000;
  return sessions
    .filter((session) => {
      const updated = Date.parse(session.updated_at || '');
      return Number.isNaN(updated) || updated >= cutoff;
    })
    .sort((a, b) => Date.parse(b.updated_at || '') - Date.parse(a.updated_at || ''))
    .slice(0, limit)
    .map((session) => {
      const live = events.get(session.id) || emptyDeskState();
      return {
        id: session.id,
        title: session.title || 'Untitled session',
        mode: session.mode || 'general',
        model: session.active_model || '',
        parentId: session.metadata?.parent_session || null,
        state: live.state,
        tool: live.tool,
        approval: live.approval,
        approvalId: live.approvalId || undefined,
        artifacts: live.artifacts,
        updatedAt: session.updated_at,
      };
    });
}

const IDLE_TO_SLEEP_MS = 10 * 60 * 1000;

const WEEKDAY_NUMBERS: Record<string, number> = {
  Mon: 1,
  Tue: 2,
  Wed: 3,
  Thu: 4,
  Fri: 5,
  Sat: 6,
  Sun: 7,
};

/** Wall-clock weekday/minute for `now`, in the agent's timezone when set. */
function localClock(
  now: Date,
  timezone?: string | null,
): { weekday: number; minutes: number } | null {
  const tz = timezone?.trim();
  if (!tz) {
    return {
      weekday: now.getDay() === 0 ? 7 : now.getDay(),
      minutes: now.getHours() * 60 + now.getMinutes(),
    };
  }
  try {
    const parts = new Intl.DateTimeFormat('en-US', {
      timeZone: tz,
      weekday: 'short',
      hour: '2-digit',
      minute: '2-digit',
      hour12: false,
    }).formatToParts(now);
    const value = (type: string) =>
      parts.find((part) => part.type === type)?.value || '';
    const weekday = WEEKDAY_NUMBERS[value('weekday')];
    const hours = Number(value('hour')) % 24;
    const minutes = Number(value('minute'));
    if (!weekday || !Number.isFinite(hours) || !Number.isFinite(minutes)) return null;
    return { weekday, minutes: hours * 60 + minutes };
  } catch {
    // Unknown IANA name: fall back to the browser's clock.
    return null;
  }
}

/** Whether an agent's working-hours window contains `now`. */
export function agentOnShift(agent: Agent, now = new Date()): boolean {
  const days = agent.schedule?.days ?? [];
  const clock = localClock(now, agent.schedule?.timezone);
  const fallback = clock === null;
  const weekday = fallback
    ? now.getDay() === 0
      ? 7
      : now.getDay()
    : clock.weekday;
  if (days.length > 0 && !days.includes(weekday)) return false;
  const [startHour, startMinute] = (agent.schedule?.start || '09:00')
    .split(':')
    .map((value) => Number(value));
  const [endHour, endMinute] = (agent.schedule?.end || '18:00')
    .split(':')
    .map((value) => Number(value));
  if (![startHour, startMinute, endHour, endMinute].every(Number.isFinite)) return true;
  const start = startHour * 60 + startMinute;
  const end = endHour * 60 + endMinute;
  const minutes = fallback
    ? now.getHours() * 60 + now.getMinutes()
    : clock.minutes;
  return start <= end ? minutes >= start && minutes < end : minutes >= start || minutes < end;
}

/**
 * Build one desk per persisted agent. Each agent's most recent conversation
 * supplies the live state; an agent with no recent work sleeps during its shift
 * and shows as off-shift outside its working hours. Live subagent sessions that
 * belong to an agent also get a desk in the Subagent Wing, so the parent's
 * recent conversation is never mistaken for the agent's latest child.
 */
export function desksFromAgents(
  agents: Agent[],
  sessions: Session[],
  events: Map<string, DeskEventState>,
  now = Date.now(),
): Desk[] {
  const nowDate = new Date(now);
  const byId = (a: Session, b: Session) => Date.parse(b.updated_at || '') - Date.parse(a.updated_at || '');
  const cutoff = now - 24 * 60 * 60 * 1000;
  const live = (session?: Session) => (session ? events.get(session.id) || emptyDeskState() : emptyDeskState());

  // Running child sessions are rendered as desks in the wing instead of being
  // folded into the parent's state. Finished children from the last 24 hours
  // are kept so the wing still shows recent delegations.
  const childSessions = sessions
    .filter((session) => {
      const updated = Date.parse(session.updated_at || '');
      return Boolean(session.metadata?.parent_session) && (Number.isNaN(updated) || updated >= cutoff);
    })
    .sort(byId);
  // Active delegations per parent conversation: the workload badge an agent
  // desk shows next to its own state.
  const activeChildren = new Map<string, number>();
  for (const session of childSessions) {
    const parent = session.metadata?.parent_session;
    if (!parent) continue;
    if (ACTIVE_STATES.has(live(session).state)) {
      activeChildren.set(parent, (activeChildren.get(parent) || 0) + 1);
    }
  }

  const desks: Desk[] = agents
    .filter((agent) => agent.enabled)
    .map((agent) => {
      const own = sessions
        .filter((session) => session.agent_id === agent.id && !session.metadata?.parent_session)
        .sort(byId);
      const session = own[0];
      const current = live(session);
      const onShift = agentOnShift(agent, nowDate);
      let state: DeskState = current.state;
      if (!session) {
        state = onShift ? 'sleeping' : 'off_shift';
      } else if (state === 'idle' && !onShift) {
        state = 'off_shift';
      } else if (state === 'completed') {
        const updated = Date.parse(session.updated_at || '');
        if (onShift && !Number.isNaN(updated) && now - updated > IDLE_TO_SLEEP_MS) {
          state = 'sleeping';
        }
      }
      return {
        id: agent.id,
        title: agent.name,
        mode: 'agent',
        model: agent.model,
        parentId: null,
        state,
        tool: current.tool,
        approval: current.approval,
        approvalId: current.approvalId || undefined,
        artifacts: current.artifacts,
        updatedAt: session?.updated_at || agent.updated_at,
        agentId: agent.id,
        role: agent.role,
        color: agent.color,
        sessionId: session?.id,
        onShift,
        workHours: `${agent.schedule?.start || '09:00'}–${agent.schedule?.end || '18:00'}`,
        children: activeChildren.get(session?.id || '') || undefined,
      };
    });

  for (const session of childSessions) {
    const current = live(session);
    const parentAgent = agents.find((agent) => agent.id === session.agent_id);
    desks.push({
      id: session.id,
      title: session.title || parentAgent?.name || 'Subagent session',
      mode: session.mode || 'general',
      model: session.active_model || '',
      parentId: session.metadata?.parent_session || null,
      state: current.state,
      tool: current.tool,
      approval: current.approval,
      approvalId: current.approvalId || undefined,
      artifacts: current.artifacts,
      updatedAt: session.updated_at,
      agentId: session.agent_id || undefined,
      sessionId: session.id,
    });
  }
  return desks;
}
