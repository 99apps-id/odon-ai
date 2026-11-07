import React, { useEffect, useRef, useState } from 'react';
import { AgentEvent, Agent, ApprovalRequest, Session } from '../types';
import { api } from '../api';
import { AgentEditorModal } from './AgentEditorModal';
import { MarkdownView } from './MarkdownView';
import {
  ACTIVE_STATES,
  AREA_AGENT,
  AREA_ORDER,
  AREA_SUBAGENT,
  Desk,
  DeskEventState,
  STATE_COLORS,
  STATE_LABELS,
  applyOfficeEvent,
  areaFor,
  desksFromAgents,
  desksFromSessions,
  roomFor,
} from './office/model';
import { DIRECTOR_ID, OfficeScene, createOfficeScene } from './office/scene';

interface AgentsOfficeViewProps {
  onOpenSession?: (id: string, title: string) => void;
}

const AREA_SUBTITLES: Record<string, string> = {
  [AREA_AGENT]: 'top-level sessions',
  [AREA_SUBAGENT]: 'delegated workers',
};

/**
 * A live isometric office over real sessions. It never invents agent names:
 * desks are sessions, the floor and wing are agent/subagent areas, and every
 * badge is driven by the daemon event stream. The director's desk is the
 * operator.
 */
export default function AgentsOfficeView({ onOpenSession }: AgentsOfficeViewProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const sceneRef = useRef<OfficeScene | null>(null);
  const agentsRef = useRef<Agent[]>([]);
  const sessionsRef = useRef<Session[]>([]);
  const eventsRef = useRef<Map<string, DeskEventState>>(new Map());
  const visibleIdsRef = useRef<Set<string>>(new Set());
  const rafRef = useRef(0);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const hoveredIdRef = useRef('');

  const [retryKey, setRetryKey] = useState(0);
  const [desks, setDesks] = useState<Desk[]>([]);
  const [selectedId, setSelectedId] = useState('');
  const [hoveredId, setHoveredId] = useState('');
  const [snippetTick, setSnippetTick] = useState(0);
  const [online, setOnline] = useState(true);
  const [streaming, setStreaming] = useState(false);
  const [webglError, setWebglError] = useState('');
  const [agentSessions, setAgentSessions] = useState<Session[]>([]);
  const [editingAgent, setEditingAgent] = useState<Agent | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [lightMode, setLightMode] = useState<'auto' | 'day' | 'night'>('auto');
  const [query, setQuery] = useState('');
  const [approvals, setApprovals] = useState<ApprovalRequest[]>([]);
  const [snapshotUrl, setSnapshotUrl] = useState('');
  const [snapshotError, setSnapshotError] = useState('');
  const [resolvingApproval, setResolvingApproval] = useState(false);
  const [approvalFeedback, setApprovalFeedback] = useState('');

  const renderDesks = () => {
    const list =
      agentsRef.current.length > 0
        ? desksFromAgents(agentsRef.current, sessionsRef.current, eventsRef.current)
        : desksFromSessions(sessionsRef.current, eventsRef.current);
    // Event deltas arrive keyed by session id; agent desks are keyed by agent
    // id, so both ids are visible to the tooltip gate.
    const visible = new Set<string>();
    for (const desk of list) {
      visible.add(desk.id);
      if (desk.sessionId) visible.add(desk.sessionId);
    }
    visibleIdsRef.current = visible;
    setDesks(list);
    sceneRef.current?.sync(list);
  };

  const refreshApprovals = async () => {
    try {
      const rows = await api.listApprovals();
      setApprovals(rows);
    } catch {
      // Approvals are best-effort in the office; the center can fetch them itself.
    }
  };

  /**
   * Anchor the tooltip near the cursor without re-rendering the whole office
   * on every pointer move; React only re-renders when the hovered desk changes.
   */
  const handleHover = (id: string, clientX: number, clientY: number) => {
    const element = tooltipRef.current;
    if (element) {
      const left = Math.min(clientX + 16, window.innerWidth - 336);
      const top = Math.min(clientY + 16, window.innerHeight - 220);
      element.style.left = `${Math.max(12, left)}px`;
      element.style.top = `${Math.max(12, top)}px`;
    }
    if (id !== hoveredIdRef.current) {
      hoveredIdRef.current = id;
      setHoveredId(id);
    }
  };

  useEffect(() => {
    if (!hoveredId) return;
    // Reasoning and answers stream token by token; refresh the tooltip on a
    // slow tick instead of re-rendering the scene per token.
    const timer = window.setInterval(() => setSnippetTick((value) => value + 1), 600);
    return () => window.clearInterval(timer);
  }, [hoveredId]);

  const scheduleRender = () => {
    if (rafRef.current) return;
    rafRef.current = window.requestAnimationFrame(() => {
      rafRef.current = 0;
      renderDesks();
    });
  };

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    // Reset container and create a pristine canvas element for WebGL initialization
    container.innerHTML = '';
    const canvas = document.createElement('canvas');
    canvas.className = 'h-full w-full block';
    canvas.setAttribute('aria-label', 'Agents Office');
    container.appendChild(canvas);

    try {
      sceneRef.current = createOfficeScene(
        canvas,
        (id) => {
          setSelectedId(id);
          sceneRef.current?.select(id);
        },
        handleHover,
      );
      setWebglError('');
    } catch (error: any) {
      console.error('[AgentsOfficeView] WebGL creation error:', error);
      setWebglError(error?.message || 'WebGL is not available in this window.');
    }

    return () => {
      sceneRef.current?.dispose();
      sceneRef.current = null;
      if (canvas.parentNode) {
        canvas.remove();
      }
    };
  }, [retryKey]);

  useEffect(() => {
    let cancelled = false;

    const refresh = async () => {
      const [agentsResult, sessionsResult] = await Promise.allSettled([
        api.listAgents(),
        api.listSessions({ includeSubagents: true }),
      ]);
      if (cancelled) return;
      let reachable = false;
      if (agentsResult.status === 'fulfilled') {
        agentsRef.current = agentsResult.value;
        reachable = true;
      }
      if (sessionsResult.status === 'fulfilled') {
        sessionsRef.current = sessionsResult.value;
        reachable = true;
      }
      if (!reachable) {
        setOnline(false);
        return;
      }
      setOnline(true);
      renderDesks();
    };

    const unsubscribe = api.subscribeAllEvents(
      (event: AgentEvent) => {
        if (event.type === 'approval_requested' || event.type === 'approval_resolved') {
          void refreshApprovals();
        }
        // Streaming text/thought for a session that is not on screen is pure
        // overhead; state/tool/approval events are still captured for every id.
        if (event.type === 'text_delta' || event.type === 'thought_delta') {
          const id = event.session_id || event.payload.session_id || '';
          if (!visibleIdsRef.current.has(id)) return;
        }
        if (applyOfficeEvent(eventsRef.current, event)) scheduleRender();
      },
      () => {
        setStreaming(true);
        void refresh();
        void refreshApprovals();
      },
      () => setStreaming(false),
    );

    setStreaming(true);
    void refresh();
    void refreshApprovals();
    const timer = window.setInterval(refresh, 5000);
    const approvalTimer = window.setInterval(refreshApprovals, 8000);

    return () => {
      cancelled = true;
      window.clearInterval(timer);
      window.clearInterval(approvalTimer);
      if (rafRef.current) window.cancelAnimationFrame(rafRef.current);
      rafRef.current = 0;
      unsubscribe();
    };
    // Refs and setters are stable; the stream is opened once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const selected = desks.find((desk) => desk.id === selectedId) || null;
  const selectedSessionId = selected?.sessionId || selected?.id || '';
  const hoveredDesk = desks.find((desk) => desk.id === hoveredId) || null;
  const hoveredIsDirector = hoveredId === DIRECTOR_ID;
  // `desks` is a fresh array every render, but the live event map mutates in
  // place as tokens stream. Look up through both ids so the rAF / tick
  // re-render picks up the newest snippet.
  const hoveredLiveId = hoveredDesk?.sessionId || hoveredDesk?.id || '';
  const hoveredLive = hoveredLiveId ? eventsRef.current.get(hoveredLiveId) : undefined;
  const hoveredThought = (hoveredLive?.thought || '').trim();
  const hoveredAnswer = (hoveredLive?.answer || '').trim();
  const activeCount = desks.filter((desk) => ACTIVE_STATES.has(desk.state)).length;
  const approvalCount = desks.filter((desk) => desk.approval).length;

  // Per-agent conversation history for the selected desk.
  useEffect(() => {
    if (!selected?.agentId) {
      setAgentSessions([]);
      return;
    }
    let cancelled = false;
    api
      .listAgentSessions(selected.agentId)
      .then((rows) => {
        if (!cancelled) setAgentSessions(rows);
      })
      .catch(() => {
        if (!cancelled) setAgentSessions([]);
      });
    return () => {
      cancelled = true;
    };
    // Depend on the agent id only: `desks` is a fresh array on every poll/render,
    // so including it refetched the whole history several times a second.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected?.agentId]);

  const openDeskChat = async (desk: Desk) => {
    if (!onOpenSession) return;
    if (!desk.agentId) {
      onOpenSession(desk.id, desk.title);
      return;
    }
    try {
      if (desk.sessionId) {
        onOpenSession(desk.sessionId, desk.title);
        return;
      }
      const session = await api.createAgentSession(desk.agentId);
      onOpenSession(session.id, session.title || desk.title);
    } catch {
      // The office stays usable when the daemon refuses a new session.
    }
  };

  const startNewChat = async (desk: Desk) => {
    if (!desk.agentId || !onOpenSession) return;
    try {
      const session = await api.createAgentSession(desk.agentId);
      setAgentSessions((prev) => [session, ...prev]);
      onOpenSession(session.id, session.title || desk.title);
    } catch {
      // Ignore: the header button is optional.
    }
  };

  const editSelectedAgent = () => {
    const agent = agentsRef.current.find(
      (row) => row.id === (selected?.agentId || selected?.id),
    );
    setEditingAgent(agent || null);
    setEditorOpen(true);
  };

  const areas = new Map<string, Desk[]>();
  for (const desk of desks) {
    const area = areaFor(desk);
    const bucket = areas.get(area) || [];
    bucket.push(desk);
    areas.set(area, bucket);
  }
  const orderedAreas = [...areas.entries()].sort(
    (a, b) => AREA_ORDER.indexOf(a[0]) - AREA_ORDER.indexOf(b[0]),
  );
  const filteredAreas = orderedAreas.map(
    ([area, items]) =>
      [
        area,
        items.filter((desk) => {
          const q = query.trim().toLowerCase();
          if (!q) return true;
          return (
            desk.title.toLowerCase().includes(q) ||
            desk.id.toLowerCase().includes(q) ||
            (desk.role || '').toLowerCase().includes(q) ||
            STATE_LABELS[desk.state].toLowerCase().includes(q)
          );
        }),
      ] as [string, Desk[]],
  );

  const selectDesk = (id: string) => {
    setSelectedId(id);
    sceneRef.current?.select(id);
  };

  const resolveDeskApproval = async (
    approvalId: string,
    decision: 'approve_once' | 'deny',
  ) => {
    setResolvingApproval(true);
    setApprovalFeedback('');
    try {
      const ok = await api.resolveApproval(approvalId, decision);
      // The approval_resolved event clears the badge on the next fold; the
      // feedback only covers a refused decision.
      if (!ok) setApprovalFeedback('The daemon refused the decision.');
      else {
        setApprovals((rows) => rows.filter((row) => row.id !== approvalId));
        void refreshApprovals();
      }
    } catch {
      setApprovalFeedback('Could not reach the daemon.');
    } finally {
      setResolvingApproval(false);
    }
  };

  const applyLightMode = (mode: 'auto' | 'day' | 'night') => {
    setLightMode(mode);
    sceneRef.current?.setLightMode(mode);
  };

  return (
    <div className="flex h-full min-h-0 gap-3">
      <div className="relative flex-1 min-w-0 overflow-hidden rounded-xl border border-neutral-200 bg-sand-50">
        <div ref={containerRef} className="h-full w-full" />

        <div className="pointer-events-none absolute left-3 top-3 flex flex-wrap items-center gap-2">
          <span className="rounded-full border border-neutral-200 bg-white/90 px-2.5 py-1 text-[11px] font-semibold text-neutral-800 shadow-2xs">
            Agents Office
          </span>
          <span
            className={`rounded-full border px-2.5 py-1 text-[10px] font-mono uppercase tracking-wide ${
              !online
                ? 'border-clay-300 bg-clay-50 text-clay-800'
                : streaming
                  ? 'border-emerald-200 bg-emerald-50 text-emerald-700'
                  : 'border-amber-200 bg-amber-50 text-amber-800'
            }`}
          >
            {!online ? 'offline' : streaming ? 'live' : 'polling'}
          </span>
          <span className="rounded-full border border-neutral-200 bg-white/90 px-2.5 py-1 text-[10px] font-mono text-neutral-600 shadow-2xs">
            {desks.length} desks · {activeCount} active
            {approvalCount > 0 ? ` · ${approvalCount} approval` : ''}
          </span>
        </div>

        <div className="absolute right-3 top-11 flex items-center gap-1.5">
          <button
            type="button"
            onClick={() => {
              setEditingAgent(null);
              setEditorOpen(true);
            }}
            className="rounded-lg border border-neutral-200/80 bg-white/85 px-2 py-1 text-[10px] font-medium text-neutral-600 shadow-2xs transition-colors hover:bg-white"
          >
            + Agent
          </button>
          <div className="flex items-center rounded-lg border border-neutral-200/80 bg-white/85 p-0.5 shadow-2xs">
            {(['auto', 'day', 'night'] as const).map((mode) => (
              <button
                key={mode}
                type="button"
                onClick={() => applyLightMode(mode)}
                className={`rounded-md px-2 py-1 text-[10px] font-medium transition-colors ${
                  lightMode === mode
                    ? 'bg-blue-600 text-white'
                    : 'text-neutral-600 hover:bg-neutral-100'
                }`}
                title={
                  mode === 'auto'
                    ? 'Follow the operator clock'
                    : `Force ${mode} lighting`
                }
              >
                {mode === 'auto' ? 'Auto' : mode === 'day' ? '☀ Day' : '☾ Night'}
              </button>
            ))}
          </div>
          <button
            type="button"
            onClick={() => sceneRef.current?.reset()}
            className="rounded-lg border border-neutral-200/80 bg-white/85 px-2 py-1 text-[10px] font-medium text-neutral-600 shadow-2xs transition-colors hover:bg-white"
          >
            Reset view
          </button>
          <button
            type="button"
            onClick={() => {
              try {
                const url = sceneRef.current?.snapshot();
                if (!url) throw new Error('no image');
                setSnapshotUrl(url);
                setSnapshotError('');
              } catch (error: any) {
                setSnapshotError(error?.message || 'snapshot failed');
              }
            }}
            className="rounded-lg border border-neutral-200/80 bg-white/85 px-2 py-1 text-[10px] font-medium text-neutral-600 shadow-2xs transition-colors hover:bg-white"
            title="Preview / download a PNG snapshot of the office"
          >
            📷 Snapshot
          </button>
        </div>

        {(snapshotUrl || snapshotError) && (
          <div className="absolute right-3 top-12 z-10 w-64 rounded-xl border border-neutral-200 bg-white/95 p-2 shadow-xl backdrop-blur-sm">
            <div className="mb-1.5 flex items-center justify-between">
              <span className="text-[11px] font-semibold text-neutral-800">Snapshot</span>
              <button
                type="button"
                onClick={() => {
                  setSnapshotUrl('');
                  setSnapshotError('');
                }}
                className="text-[10px] text-neutral-500 hover:text-neutral-700"
              >
                Close
              </button>
            </div>
            {snapshotUrl ? (
              <>
                <img src={snapshotUrl} alt="Office snapshot" className="w-full rounded-lg border border-neutral-100" />
                <a
                  href={snapshotUrl}
                  download="agents-office.png"
                  className="mt-1.5 block rounded-lg bg-blue-600 px-2 py-1 text-center text-[11px] font-semibold text-white hover:bg-blue-700"
                >
                  Download PNG
                </a>
              </>
            ) : (
              <p className="text-[11px] text-clay-700">{snapshotError}</p>
            )}
          </div>
        )}

        <div className="pointer-events-none absolute bottom-3 left-3 flex flex-wrap items-center gap-2 rounded-xl border border-neutral-200/80 bg-white/85 px-2.5 py-1.5 text-[10px] text-neutral-600 shadow-2xs">
          {(['thinking', 'executing_tool', 'awaiting_approval', 'completed', 'failed', 'sleeping', 'off_shift'] as const).map(
            (state) => (
              <span key={state} className="inline-flex items-center gap-1">
                <span
                  className="inline-block h-2 w-2 rounded-full"
                  style={{ backgroundColor: STATE_COLORS[state] }}
                />
                {STATE_LABELS[state]}
              </span>
            ),
          )}
        </div>

        {webglError ? (
          <div className="absolute inset-0 overflow-y-auto bg-sand-50/95 p-5">
            <div className="mx-auto max-w-xl space-y-2">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2 text-xs font-semibold text-neutral-800">
                  <span className="h-2 w-2 rounded-full bg-clay-500" />
                  The 3D view is unavailable in this window
                </div>
                <button
                  type="button"
                  onClick={() => setRetryKey((k) => k + 1)}
                  className="px-2.5 py-1 text-[11px] font-medium text-blue-600 bg-blue-50 hover:bg-blue-100 rounded-lg transition-colors cursor-pointer"
                >
                  Retry 3D View
                </button>
              </div>
              <p className="text-[11px] leading-relaxed text-neutral-500">
                {webglError} The office data is still live; use the list on the right or open a
                session in chat.
              </p>
              <div className="grid gap-2 pt-1 sm:grid-cols-2">
                {desks.map((desk) => (
                  <button
                    key={desk.id}
                    type="button"
                    onClick={() => selectDesk(desk.id)}
                    className={`rounded-xl border bg-white p-2.5 text-left transition-colors ${
                      desk.id === selectedId
                        ? 'border-blue-300 bg-blue-50/40'
                        : 'border-neutral-200 hover:bg-neutral-100'
                    }`}
                  >
                    <div className="flex items-center gap-1.5">
                      <span
                        className="h-2 w-2 shrink-0 rounded-full"
                        style={{ backgroundColor: STATE_COLORS[desk.state] }}
                      />
                      <span className="truncate text-[11px] font-semibold text-neutral-800">
                        {desk.title}
                      </span>
                    </div>
                    <div className="mt-1 text-[10px] text-neutral-500">
                      {roomFor(desk.mode)} · {STATE_LABELS[desk.state]}
                    </div>
                  </button>
                ))}
              </div>
            </div>
          </div>
        ) : desks.length === 0 ? (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            <div className="max-w-sm rounded-xl border border-neutral-200 bg-white/90 px-4 py-3 text-center shadow-2xs">
              <div className="text-xs font-semibold text-neutral-700">The office is quiet</div>
              <p className="mt-1 text-[11px] leading-relaxed text-neutral-500">
                No sessions were updated in the last 24 hours. Start a task in chat and its desk
                will appear here.
              </p>
            </div>
          </div>
        ) : null}
      </div>

      <aside className="flex w-72 shrink-0 flex-col gap-3 overflow-y-auto rounded-xl border border-neutral-200 bg-white p-3">
        <section>
          <h2 className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-neutral-400">
            Areas
          </h2>
          <div className="mb-2 flex items-center gap-1.5">
            <span className="text-neutral-400" aria-hidden="true">
              ⌕
            </span>
            <input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder="Filter desks by name, state or role…"
              className="h-7 w-full rounded-lg border border-neutral-200 bg-sand-50 px-2 text-[11px] outline-none focus:border-blue-300 focus:bg-white"
              aria-label="Filter desks"
            />
            {query && (
              <button
                type="button"
                onClick={() => setQuery('')}
                className="shrink-0 rounded-md border border-neutral-200 bg-white px-1.5 py-0.5 text-[10px] text-neutral-500 hover:bg-neutral-100"
              >
                Clear
              </button>
            )}
          </div>
          {filteredAreas.length === 0 ? (
            <p className="text-[11px] text-neutral-400">No desks yet.</p>
          ) : (
            <div className="space-y-2">
              {filteredAreas.map(([area, items]) => {
                const visible = items.filter((_, index) => index < 50);
                return (
                <div key={area} className="rounded-xl border border-neutral-200 bg-sand-50/60 p-2">
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <span className="text-[11px] font-semibold text-neutral-700">{area}</span>
                    <span className="font-mono text-[9px] text-neutral-400">
                      {items.length} desk{items.length === 1 ? '' : 's'}
                    </span>
                  </div>
                  <div className="mb-1 text-[9px] text-neutral-400">
                    {AREA_SUBTITLES[area] || 'sessions'}
                  </div>
                  <div className="space-y-0.5">
                    {visible.map((desk) => (
                      <button
                        key={desk.id}
                        type="button"
                        onClick={() => {
                          selectDesk(desk.id);
                          sceneRef.current?.focus(desk.id);
                        }}
                        className={`flex w-full items-center gap-1.5 rounded-lg px-1.5 py-1 text-left transition-colors ${
                          desk.id === selectedId
                            ? 'bg-blue-50 text-blue-800'
                            : 'text-neutral-600 hover:bg-white'
                        }`}
                      >
                        <span
                          className="h-1.5 w-1.5 shrink-0 rounded-full"
                          style={{ backgroundColor: STATE_COLORS[desk.state] }}
                        />
                        <span className="min-w-0 flex-1 truncate text-[10.5px]">
                          {desk.title}
                        </span>
                        {desk.children ? (
                          <span
                            className="shrink-0 rounded-md bg-blue-100 px-1 font-mono text-[9px] font-semibold text-blue-700"
                            title={`${desk.children} active delegation${desk.children === 1 ? '' : 's'}`}
                          >
                            ×{desk.children}
                          </span>
                        ) : null}
                        <span className="shrink-0 font-mono text-[9px] text-neutral-400">
                          {STATE_LABELS[desk.state]}
                        </span>
                      </button>
                    ))}
                  </div>
                </div>
              );
              })}
            </div>
          )}
          <div className="mt-2 rounded-xl border border-amber-200/70 bg-amber-50/50 p-2">
            <div className="flex items-center justify-between gap-2">
              <span className="text-[11px] font-semibold text-amber-900">
                Director's Office
              </span>
              <span className="font-mono text-[9px] text-amber-700">you</span>
            </div>
            <p className="mt-0.5 text-[9px] leading-snug text-amber-800/70">
              Your desk in the back office; the floor and wing are live sessions.
            </p>
            <div className="mt-1.5 flex gap-1.5">
              <button
                type="button"
                onClick={() => {
                  setSelectedId(DIRECTOR_ID);
                  sceneRef.current?.focus(DIRECTOR_ID);
                }}
                className="flex-1 rounded-lg border border-amber-300/70 bg-white/80 px-2 py-1 text-[10px] font-semibold text-amber-900 transition-colors hover:bg-white"
              >
                Focus director
              </button>
              <button
                type="button"
                onClick={() => applyLightMode('night')}
                className="flex-1 rounded-lg border border-amber-300/70 bg-white/80 px-2 py-1 text-[10px] font-semibold text-amber-900 transition-colors hover:bg-white"
              >
                Lights on
              </button>
            </div>
          </div>

          {approvals.length > 0 && (
            <div className="rounded-xl border border-clay-200 bg-clay-50/40 p-2">
              <div className="mb-1 flex items-center justify-between">
                <span className="text-[11px] font-semibold text-clay-900">
                  Pending approvals
                </span>
                <span className="font-mono text-[9px] text-clay-700">{approvals.length}</span>
              </div>
              <div className="space-y-1">
                {approvals.slice(0, 8).map((approval) => {
                    const desk = desks.find(
                      (row) =>
                        row.sessionId === approval.session_id ||
                        row.id === approval.session_id,
                    );
                    return (
                    <div
                      key={approval.id}
                      className="flex items-center gap-1.5 rounded-lg bg-white/80 px-1.5 py-1"
                    >
                      <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-clay-500" />
                      <span className="min-w-0 flex-1 truncate text-[10.5px] text-neutral-700">
                        {approval.tool_name || approval.description || approval.session_id}
                      </span>
                      {desk && (
                        <button
                          type="button"
                          onClick={() => {
                            selectDesk(desk.id);
                            sceneRef.current?.focus(desk.id);
                          }}
                          className="shrink-0 rounded-md border border-neutral-200 bg-white px-1.5 py-0.5 text-[9px] font-semibold text-neutral-600 hover:bg-neutral-100"
                        >
                          View
                        </button>
                      )}
                      <button
                        type="button"
                        disabled={resolvingApproval}
                        onClick={() =>
                          void resolveDeskApproval(approval.id, 'approve_once')
                        }
                        className="shrink-0 rounded-md bg-emerald-600 px-1.5 py-0.5 text-[9px] font-semibold text-white hover:bg-emerald-700 disabled:opacity-50"
                      >
                        Approve
                      </button>
                      <button
                        type="button"
                        disabled={resolvingApproval}
                        onClick={() => void resolveDeskApproval(approval.id, 'deny')}
                        className="shrink-0 rounded-md border border-clay-300 bg-white px-1.5 py-0.5 text-[9px] font-semibold text-clay-800 hover:bg-clay-100 disabled:opacity-50"
                      >
                        Reject
                      </button>
                    </div>
                  );
                })}
              </div>
              {approvalFeedback && (
                <p className="mt-1 text-[9.5px] text-clay-800">{approvalFeedback}</p>
              )}
            </div>
          )}
        </section>

        <section className="border-t border-neutral-100 pt-3">
          <h2 className="mb-2 text-[10px] font-semibold uppercase tracking-wider text-neutral-400">
            Selected desk
          </h2>
          {!selected ? (
            <p className="text-[11px] leading-relaxed text-neutral-400">
              Click a desk in the office or a row above.
            </p>
          ) : (
            <div className="space-y-2">
              <div className="flex flex-wrap items-center gap-1.5">
                <span className="text-xs font-semibold text-neutral-900">{selected.title}</span>
                {selected.role && (
                  <span className="rounded-full bg-neutral-100 px-1.5 py-0.5 text-[9px] font-medium text-neutral-500">
                    {selected.role}
                  </span>
                )}
              </div>
              <dl className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-1 text-[10.5px]">
                {[
                  ['Area', areaFor(selected)],
                  ['Room', selected.role || roomFor(selected.mode)],
                  ['State', STATE_LABELS[selected.state]],
                  ...(selected.agentId
                    ? ([
                        ['Shift', selected.onShift ? 'on shift' : 'off shift'],
                        ['Hours', selected.workHours || '—'],
                      ] as Array<[string, string]>)
                    : []),
                  ...(selected.children
                    ? ([['Delegations', String(selected.children)]] as Array<[string, string]>)
                    : []),
                  ['Model', selected.model || '—'],
                  ['Tool', selected.tool || '—'],
                  ['Approval', selected.approval || '—'],
                  ['Artifacts', selected.artifacts.length ? selected.artifacts.join(', ') : '—'],
                  ...(selected.parentId ? [['Parent', selected.parentId] as [string, string]] : []),
                  ['Session', selectedSessionId || '—'],
                ].map(([label, value]) => (
                  <React.Fragment key={label}>
                    <dt className="text-neutral-400">{label}</dt>
                    <dd className="m-0 break-all font-mono text-neutral-700">{value}</dd>
                  </React.Fragment>
                ))}
              </dl>
              <div className="flex gap-1.5">
                <button
                  type="button"
                  onClick={() => sceneRef.current?.focus(selected.id)}
                  className="flex-1 rounded-lg border border-neutral-200 bg-white px-2.5 py-1.5 text-[11px] font-semibold text-neutral-700 transition-colors hover:bg-neutral-100"
                >
                  Focus
                </button>
                {onOpenSession && (
                  <button
                    type="button"
                    onClick={() => void openDeskChat(selected)}
                    className="flex-1 rounded-lg bg-blue-600 px-2.5 py-1.5 text-[11px] font-semibold text-white transition-colors hover:bg-blue-700"
                  >
                    Open chat
                  </button>
                )}
              </div>
              {selected.approvalId && (
                <div className="rounded-xl border border-clay-200 bg-clay-50/60 p-2">
                  <div className="text-[10px] font-semibold text-clay-800">
                    Waiting on approval: {selected.approval}
                  </div>
                  <div className="mt-1.5 flex gap-1.5">
                    <button
                      type="button"
                      disabled={resolvingApproval}
                      onClick={() =>
                        void resolveDeskApproval(selected.approvalId!, 'approve_once')
                      }
                      className="flex-1 rounded-lg bg-emerald-600 px-2 py-1 text-[10.5px] font-semibold text-white transition-colors hover:bg-emerald-700 disabled:opacity-50"
                    >
                      Approve once
                    </button>
                    <button
                      type="button"
                      disabled={resolvingApproval}
                      onClick={() => void resolveDeskApproval(selected.approvalId!, 'deny')}
                      className="flex-1 rounded-lg border border-clay-300 bg-white px-2 py-1 text-[10.5px] font-semibold text-clay-800 transition-colors hover:bg-clay-100 disabled:opacity-50"
                    >
                      Reject
                    </button>
                  </div>
                  {approvalFeedback && (
                    <p className="mt-1 text-[9.5px] text-clay-800">{approvalFeedback}</p>
                  )}
                </div>
              )}
              <div className="flex gap-1.5">
                {selected.agentId && onOpenSession && (
                  <button
                    type="button"
                    onClick={() => void startNewChat(selected)}
                    className="flex-1 rounded-lg border border-neutral-200 bg-white px-2.5 py-1.5 text-[11px] font-semibold text-neutral-700 transition-colors hover:bg-neutral-100"
                  >
                    New chat
                  </button>
                )}
                <button
                  type="button"
                  onClick={editSelectedAgent}
                  className="flex-1 rounded-lg border border-neutral-200 bg-white px-2.5 py-1.5 text-[11px] font-semibold text-neutral-700 transition-colors hover:bg-neutral-100"
                >
                  Edit agent
                </button>
              </div>

              {(selected.log?.length || 0) > 0 && (
                <div>
                  <div className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-neutral-400">
                    Recent activity
                  </div>
                  <div className="max-h-28 space-y-0.5 overflow-y-auto rounded-lg border border-neutral-100 bg-sand-50/60 p-1.5">
                    {(selected.log || []).map((entry, index) => (
                      <div
                        key={`${index}-${entry}`}
                        className="font-mono text-[9.5px] leading-snug text-neutral-600"
                      >
                        {entry}
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {selected.agentId && agentSessions.length > 0 && (
                <div>
                  <div className="mb-1 text-[9px] font-semibold uppercase tracking-wider text-neutral-400">
                    History
                  </div>
                  <div className="max-h-32 space-y-0.5 overflow-y-auto">
                    {agentSessions.slice(0, 8).map((session) => (
                      <button
                        key={session.id}
                        type="button"
                        onClick={() => onOpenSession?.(session.id, session.title)}
                        className="flex w-full items-center gap-1.5 rounded-lg px-1.5 py-1 text-left text-[10.5px] text-neutral-600 transition-colors hover:bg-neutral-100"
                      >
                        <span className="min-w-0 flex-1 truncate">{session.title}</span>
                        <span className="shrink-0 font-mono text-[9px] text-neutral-400">
                          {session.updated_at
                            ? new Date(session.updated_at).toLocaleDateString()
                            : ''}
                        </span>
                      </button>
                    ))}
                  </div>
                </div>
              )}
            </div>
          )}
        </section>
      </aside>

      {/* Hover tooltip: the desk is a real session, and this is its live state.
          Positioned imperatively so pointer movement does not re-render the
          scene; React only re-renders when the hovered desk changes. */}
      <div
        ref={tooltipRef}
        data-tick={snippetTick}
        className={`pointer-events-none fixed z-50 w-80 rounded-xl border border-neutral-200 bg-white/95 p-3 text-left shadow-xl backdrop-blur-sm transition-opacity duration-100 ${
          hoveredDesk || hoveredIsDirector ? 'opacity-100' : 'opacity-0'
        }`}
      >
        {hoveredIsDirector && (
          <div className="rounded-xl border border-blue-100 bg-blue-50/60 p-3">
            <div className="flex items-center gap-1.5">
              <span className="h-2 w-2 shrink-0 rounded-full bg-blue-600" />
              <span className="min-w-0 flex-1 truncate text-[11px] font-semibold text-neutral-900">
                You · Director
              </span>
              <span className="shrink-0 font-mono text-[9px] uppercase tracking-wide text-neutral-400">
                operator
              </span>
            </div>
            <p className="mt-0.5 text-[10px] leading-snug text-neutral-500">
              The operator's desk. Watch the floor: agents keep the Office moving.
            </p>
            <div className="mt-1.5 flex flex-wrap gap-1">
              <span className="rounded-md border border-neutral-200 bg-neutral-50 px-1.5 py-0.5 font-mono text-[9px] text-neutral-600">
                camera + approvals
              </span>
              <span className="rounded-md border border-amber-200 bg-amber-50 px-1.5 py-0.5 text-[9px] text-amber-800">
                director's office
              </span>
            </div>
            <p className="mt-2 text-[9px] text-neutral-400">Click the desk to focus it.</p>
          </div>
        )}
        {hoveredDesk && (
          <div>
            <div className="flex items-start gap-1.5">
              <span
                className="mt-0.5 h-2 w-2 shrink-0 rounded-full ring-2 ring-white"
                style={{ backgroundColor: STATE_COLORS[hoveredDesk.state] }}
              />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[11px] font-semibold text-neutral-900">
                  {hoveredDesk.title}
                </div>
                <div className="mt-0.5 truncate text-[10px] text-neutral-500">
                  {areaFor(hoveredDesk)} · {hoveredDesk.role || roomFor(hoveredDesk.mode)}
                  {hoveredDesk.model ? ` · ${hoveredDesk.model}` : ''}
                  {hoveredDesk.workHours ? ` · ${hoveredDesk.workHours}` : ''}
                  {hoveredDesk.children
                    ? ` · ${hoveredDesk.children} active delegation${hoveredDesk.children === 1 ? '' : 's'}`
                    : ''}
                </div>
              </div>
              <span
                className="shrink-0 rounded-full border px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-wide"
                style={{
                  color: STATE_COLORS[hoveredDesk.state],
                  borderColor: STATE_COLORS[hoveredDesk.state] + '55',
                  backgroundColor: STATE_COLORS[hoveredDesk.state] + '14',
                }}
              >
                {STATE_LABELS[hoveredDesk.state]}
              </span>
            </div>

            {(hoveredDesk.tool || hoveredDesk.approval || (hoveredDesk.log?.length || 0) > 0) && (
              <div className="mt-2 rounded-lg border border-neutral-100 bg-neutral-50/70 p-2">
                <div className="flex items-center gap-1">
                  <span className="text-[9px] font-semibold uppercase tracking-wider text-neutral-400">
                    Activity
                  </span>
                  {ACTIVE_STATES.has(hoveredDesk.state) && (
                    <span className="ml-auto inline-flex items-center gap-1 font-mono text-[9px] text-emerald-700">
                      <span className="relative flex h-1.5 w-1.5">
                        <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-emerald-400 opacity-75" />
                        <span className="relative inline-flex h-1.5 w-1.5 rounded-full bg-emerald-600" />
                      </span>
                      Live
                    </span>
                  )}
                </div>
                {hoveredDesk.tool && (
                  <div className="mt-1 flex items-center gap-1">
                    <span className="rounded-md border border-blue-200 bg-blue-50 px-1.5 py-0.5 font-mono text-[9px] text-blue-700">
                      {hoveredDesk.tool}
                    </span>
                  </div>
                )}
                {hoveredDesk.approval && (
                  <div className="mt-1 inline-block rounded-md border border-clay-200 bg-clay-50 px-1.5 py-0.5 text-[9px] font-semibold text-clay-800">
                    approval: {hoveredDesk.approval}
                  </div>
                )}
                {(hoveredDesk.log?.length || 0) > 0 && (
                  <div className="mt-1.5 space-y-0.5">
                    {(hoveredDesk.log || []).slice(0, 3).map((entry, index) => (
                      <div
                        key={`${index}-${entry}`}
                        className="font-mono text-[9.5px] leading-snug text-neutral-500"
                      >
                        {entry}
                      </div>
                    ))}
                  </div>
                )}
              </div>
            )}

            {hoveredDesk.approvalId && (
              <div className="pointer-events-auto mt-1.5 flex gap-1.5">
                <button
                  type="button"
                  disabled={resolvingApproval}
                  onClick={() =>
                    void resolveDeskApproval(hoveredDesk.approvalId!, 'approve_once')
                  }
                  className="flex-1 rounded-lg bg-emerald-600 px-2 py-1 text-[10px] font-semibold text-white transition-colors hover:bg-emerald-700 disabled:opacity-50"
                >
                  Approve
                </button>
                <button
                  type="button"
                  disabled={resolvingApproval}
                  onClick={() => void resolveDeskApproval(hoveredDesk.approvalId!, 'deny')}
                  className="flex-1 rounded-lg border border-clay-300 bg-white px-2 py-1 text-[10px] font-semibold text-clay-800 transition-colors hover:bg-clay-100 disabled:opacity-50"
                >
                  Reject
                </button>
              </div>
            )}

            <div className="mt-2 space-y-2">
              {hoveredThought && (
                <div className="rounded-lg border border-blue-100 bg-blue-50/60 p-2">
                  <div className="flex items-center gap-1.5">
                    <span className="text-[9px] font-semibold uppercase tracking-wider text-blue-700">
                      Reasoning
                    </span>
                    <span className="inline-flex items-center gap-1 font-mono text-[9px] text-blue-600">
                      <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-blue-500" />
                      Streaming
                    </span>
                  </div>
                  <div className="mt-0.5 max-h-20 overflow-y-auto text-[10.5px] leading-snug text-blue-950/80">
                    <MarkdownView content={hoveredThought} dense preserveLineBreaks />
                  </div>
                </div>
              )}
              {hoveredAnswer && (
                <div className="rounded-lg border border-emerald-100 bg-emerald-50/70 p-2">
                  <div className="flex items-center gap-1.5">
                    <span className="text-[9px] font-semibold uppercase tracking-wider text-emerald-700">
                      Reply
                    </span>
                    <span className="inline-flex items-center gap-1 font-mono text-[9px] text-emerald-600">
                      <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-emerald-500" />
                      Streaming
                    </span>
                  </div>
                  <div className="mt-0.5 max-h-20 overflow-y-auto text-[10.5px] leading-snug text-emerald-950/80">
                    <MarkdownView content={hoveredAnswer} dense preserveLineBreaks />
                  </div>
                </div>
              )}
              {!hoveredThought && !hoveredAnswer && (
                <div className="rounded-lg border border-neutral-100 bg-neutral-50/60 px-2.5 py-2 text-center text-[10px] text-neutral-400">
                  No reasoning or reply captured yet.
                </div>
              )}
            </div>
            <div className="mt-2 text-[9px] text-neutral-400">
              Click the desk to open it in chat.
            </div>
          </div>
        )}
      </div>

      <AgentEditorModal
        agent={editingAgent}
        open={editorOpen}
        onClose={() => setEditorOpen(false)}
        onSaved={(saved) => {
          const exists = agentsRef.current.some((row) => row.id === saved.id);
          agentsRef.current = exists
            ? agentsRef.current.map((row) => (row.id === saved.id ? saved : row))
            : [...agentsRef.current, saved];
          renderDesks();
        }}
      />
    </div>
  );
}
