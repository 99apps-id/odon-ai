# Building a standalone ODON Agents Office app

Use this skill when the operator wants a browser app that visualizes ODON sessions and
subagents as an isometric office. `agents_office_create` writes a runnable starter into the
workspace; this skill is the contract for extending it correctly.

This is an original ODON design. Do not copy code, art, names, marks or text from any other
office project; build from the data and palette below.

## What the office may show (only real data)

| Office element | Real source |
|---|---|
| Room | `session.mode` (`general`, `code`, `create`, `documents`, `files`, `agents`, `automations`) |
| Desk | one session from `GET /api/sessions?include_subagents=true` |
| Nameplate | `session.title` |
| Model badge | `session.active_model` |
| Agent state | SSE `state_changed.state` (`thinking`, `executing_tool`, `awaiting_approval`, `paused`, `failed`, `completed`, `idle`) |
| Speech bubble | `tool_call_started.tool_name`, cleared by the matching `tool_call_finished.call_id` |
| Red lamp | `approval_requested`, cleared by `approval_resolved` |
| Output tray | `artifact_created.artifact.kind` |
| Child desk | `session.metadata.parent_session` + `child_index` / `child_count` |
| Child activity | `subagent_progress.phase` (`started`, `working`, `finished`, `failed`) |

Never invent departments, agent names, queue depths or a `running` status. `Session.status` is
not maintained; use live events and the `running` flag from `GET /api/sessions/{id}`.

## Data plumbing the starter already handles

1. `GET /api/health` from the app origin. Because `http://localhost:5173` is a trusted
   loopback origin, the response carries `session_token`. Adopt it in memory; never write it
   to disk or a URL.
2. `GET /api/sessions?include_subagents=true` with `Authorization: Bearer <token>`. Poll every
   5 seconds as a safety net.
3. `GET /api/events?token=<token>` for live `message` frames. On reconnect, re-fetch the
   session list: missed state changes are gone.
4. Cap the roster: keep sessions updated in the last 24 hours, running first, at most 16
   desks. Everything else stays off-stage.

If `/api/health` does not return a token (wrong origin, auth disabled, daemon down), show a
small settings card where the operator can paste a base URL and token. Do not fail silently.

## Scene and layout

- Orthographic isometric camera; one floor pod per room, desks in a grid, a central "Brain"
  core with a wire per room.
- The ODON palette is light-only: sand/neutral surfaces (`#faf8f5`, `#f2ede4`, `#e5dccd`),
  blue ink accent `#3f5b70`, clay warning `#8d5f49`, emerald success `#047857`.
- State colours: thinking blue, executing tool blue-400, awaiting approval clay, failed clay,
  completed emerald, idle neutral.
- Animate only `transform` and `opacity`; disable bobbing/rotation when
  `prefers-reduced-motion: reduce`.
- Clicking a desk selects it and shows title, model, state, last tool, pending approval and an
  "Open in ODON" link to `http://localhost:3000`. The office itself is read-only: do not send
  messages or approvals from the generated app.
- Hovering a desk shows the live tooltip (title, room, model, last tool/approval, and the
  reasoning/reply tails). The tooltip must stay in sync without recreating scene objects—update
  its text on a 600 ms tick while hovered.
- Keep the director parity: the back-office "YOU · DIRECTOR" desk is hoverable and focusable,
  and the side panel shows a director card with Focus and Lights on actions. Do not add a
  second principal.

## Project shape

```text
agents-office/
  index.html
  src/main.js      # boot, camera, render loop, selection
  src/office.js    # rooms, desks, avatars, brain, labels
  src/data.js      # health/token, sessions, SSE, event reducer
  src/style.css    # overlay panels and legend
  serve.mjs        # static server with CORS *
  README.md
```

Run it with `cd <dir>; node serve.mjs` and open `http://127.0.0.1:5173` in the Browser panel.

## Extending

- Add a room by mapping a new `session.mode`; keep the label literal, not a fabricated
  department.
- Add a visual state by reading an event that actually exists in `AgentEvent`; do not derive
  state from text deltas.
- Keep the scene rebuild cheap: update transforms/materials on `sync`, do not recreate
  geometries per event.
- Night mode is optional but should follow the desktop palette: hanging pendants, floor pools
  directly under each pendant, a darker skyline and star field, warm point lights, and a
  "Night on" pill so the toggle state is always visible.
- Keep hover and snapshot cheap: hover only moves the tooltip element in JS; Snapshot renders
  one frame and offers a PNG download. Never stream the canvas per token.
- Bound every array: sessions, artifacts per desk, tool labels. A long-running install must
  not grow the scene without limit.
