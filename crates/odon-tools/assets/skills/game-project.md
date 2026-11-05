# Building a playable browser game as a workspace project

Use this skill for any request that ends in a game the operator can play: a platformer, a
runner, a 3D exploration scene, a puzzle, or a prototype of one. The deliverable is a real
project in the workspace, not a chat snippet.

## Start from the scaffold

Call `game_scaffold` with `template: "platformer-2d"` for a canvas game with no dependencies,
or `template: "adventure-3d"` for a Three.js third-person scene. It writes a runnable project
and returns `project_dir`, the file list, and the preview command. Edit those files with
`fs_edit` / `fs_write`; do not rebuild the boilerplate by hand.

After writing, verify every file exists (`fs_list`), then start the preview:

```text
dev_server_start { command: "cd <project_dir>; node serve.mjs" }
```

Open `http://127.0.0.1:5173` in the Browser panel. The generated `serve.mjs` sends
`Access-Control-Allow-Origin: *` because the ODON preview iframe has an opaque origin; without
that header ES modules and `fetch()` fail even though the page itself loads.

## Project layout

```text
<project>/
  index.html          # canvas mount + import map (3D only)
  src/main.js         # boot, resize, input, HUD, game loop
  src/game.js         # world, entities, collision, scoring, restart
  src/style.css       # page chrome and HUD
  serve.mjs           # zero-dependency static server with CORS
  assets/models/      # .gltf files produced by model3d_create
  README.md           # controls, run command, asset notes
```

## Rules that keep the game playable in the ODON preview

- **Preview is an opaque-origin iframe.** `localStorage`, `sessionStorage`, cookies and
  `window.open` throw or are blocked. Keep state in memory; if you need a save, guard it with
  `try { localStorage.setItem(...) } catch {}` so opening the page directly still works.
- **No pointer lock, no fullscreen, no modals.** Use ordinary keyboard/mouse handlers. The
  sandbox does not grant those permissions; requesting them fails silently.
- **Use absolute CDN URLs for Three.js** in an import map:
  `<script type="importmap">{"imports":{"three":"https://cdn.jsdelivr.net/npm/three@0.170.0/build/three.module.js","three/addons/":"https://cdn.jsdelivr.net/npm/three@0.170.0/examples/jsm/"}}</script>`.
  Relative specifiers resolve against the preview document, not the dev server.
- Keep the page self-contained: no backend, no auth, no analytics. Assets live under the
  project folder and are served by `serve.mjs`.
- If you change the port, change it in `serve.mjs` (PORT env or the default) and tell the
  operator the new URL.

## Game feel checklist

1. Fixed camera or follow camera that never fights the player.
2. Delta-time movement (`dt = min(clock.getDelta(), 0.05)`) so speed does not depend on FPS.
3. A clear goal, a visible score/health HUD, and an instant restart (`R`) without reloading.
4. Collision that resolves against the previous position, not by teleporting out of geometry.
5. Keyboard input that ignores repeats and works after the canvas has focus.
6. A win/lose state that says what happened and how to play again.
7. Resize handling (`window.addEventListener('resize', ...)`) and device pixel ratio clamped
   to 2.
8. No unbounded allocations in the loop: reuse vectors and geometries.

## 3D models

Generate assets with `model3d_create` into `assets/models/`. Load them with `GLTFLoader`
(`three/addons/loaders/GLTFLoader.js`) relative to the project URL, e.g.
`new URL('./assets/models/hero.gltf', import.meta.url)`. The tool emits one flat-shaded,
vertex-coloured mesh: low-poly is the house style. See the `procedural-3d-gltf` skill for the
part grammar and proportions.

## Verify before reporting

- `dev_server_start` succeeded and `http://127.0.0.1:5173` answers.
- The page loads modules (no CORS error in the dev-server log).
- Movement, collision, scoring, win/lose and restart all work.
- The README says the run command, the controls, and where generated models live.
