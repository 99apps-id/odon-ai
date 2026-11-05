# {{TITLE}}

A small ODON game project. Real files on disk, no build step, no dependencies.

## Run it

```bash
cd {{DIR}}
node serve.mjs
```

Then open `http://127.0.0.1:{{PORT}}` in the ODON Browser panel.

`serve.mjs` serves this folder on loopback and adds `Access-Control-Allow-Origin: *`
because the ODON preview iframe has an opaque origin. It is a local development server,
not something to expose publicly.

## Files

- `index.html` — page shell and canvas.
- `src/main.js` — boot, input, HUD, loop.
- `src/game.js` — level, physics, collision, rendering.
- `src/style.css` — page chrome.
- `assets/models/` — glTF assets generated with `model3d_create`.
- `serve.mjs` — the preview server.

## Assets

Models are generated deterministically with the `model3d_create` tool and load with
`GLTFLoader` relative to the project root. Keep them low-poly and vertex-coloured; the
preview iframe cannot rely on external textures.

## Notes

- The ODON preview runs the page in a sandboxed iframe: `localStorage` may throw, popups
  are blocked, and pointer lock is unavailable. Keep state in memory.
- If you change the port, update the command and this README.
