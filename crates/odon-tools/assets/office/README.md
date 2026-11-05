# {{TITLE}}

A live isometric view of ODON sessions and delegated subagents. Real data only: rooms are
session modes, desks are sessions, and state comes from the daemon event stream.

## Run it

```bash
cd {{DIR}}
node serve.mjs
```

Open `http://127.0.0.1:{{PORT}}` in the ODON Browser panel (or a normal browser tab).

`serve.mjs` serves the app and proxies `/api/*` to the daemon. It reads the loopback token
from `~/.odon/api-token` (or `ODON_API_TOKEN`) and adds it server-side, so the browser never
holds a token and the daemon does not need a wider CORS allowlist. The daemon URL defaults
to `http://127.0.0.1:4096`; override it with `ODON_DAEMON_URL`.

## Data mapping

- **Room** — `session.mode` (`general`, `code`, `create`, `documents`, `files`, `agents`,
  `automations`). Labels are literal room names, not invented departments.
- **Desk** — one session from `GET /api/sessions?include_subagents=true` updated in the last
  24 hours, capped at 16.
- **State** — SSE `state_changed`; tool bubbles from `tool_call_started` / `tool_call_finished`;
  approval lamps from `approval_requested` / `approval_resolved`; output trays from
  `artifact_created`; child desks from `subagent_progress` and `metadata.parent_session`.
- **Live stream** — `GET /api/events` through the proxy. Polling every 5 s is a safety net;
  reconnect resyncs the session list.

- The app is deliberately read-only. It never sends messages, approvals, or tool calls.
- Hovering a desk shows the live tooltip (title, room, model, last tool/approval, and the
  reasoning/reply tails). The tooltip refreshes on a 600 ms tick while hovered.
- Day/night toggle: hanging pendants, floor pools under each lamp, a darker star skyline,
  warm point lights, and a "Night on" pill so the toggle state is always visible.
- Director desk is hoverable and focusable; the side panel shows a director card with
  Focus and Lights on actions. Snapshot renders one frame and offers a PNG download.
- Room rows filter by title, id, room, or state; clicking a row focuses the desk.

## Notes

- `Session.status` is not a live field; do not render it. Use events and the session list.
- The ODON preview iframe has an opaque origin, which is why the daemon is reached through
  `serve.mjs`; a direct browser `fetch` to the daemon would be rejected.
- Keep the roster bounded. A long-lived daemon has hundreds of sessions; this page shows the
  recent ones and leaves the rest off-stage.
- `prefers-reduced-motion` disables the bobbing avatars and the rotating core.
