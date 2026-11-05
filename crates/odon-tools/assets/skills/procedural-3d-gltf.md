# Procedural game models as glTF

Use this skill whenever a game or scene needs a 3D asset and no external model service is
available. `model3d_create` builds a deterministic, low-poly glTF 2.0 file from primitives; it
is for blockouts, props, terrain tiles, characters, and prototypes.

## Coordinate system and scale

- **Y up, right-handed, metres.** One unit is one metre.
- Put the origin at the base centre of the model (feet on `y = 0`) so it drops onto a floor
  or a game entity without a vertical offset.
- A human-scale character is about 1.7–1.8 units tall; a crate is 0.6–1.0; a tree is 3–6.
- Keep models under a few thousand triangles. Flat shading and vertex colours are intentional.

## Tool call

```json
{
  "output": "assets/models/crate.gltf",
  "name": "crate",
  "parts": [
    { "shape": "box", "position": [0, 0.4, 0], "scale": [0.8, 0.8, 0.8], "color": "#a8775f" },
    { "shape": "box", "position": [0, 0.82, 0], "scale": [0.84, 0.06, 0.84], "color": "#5d3d30" }
  ]
}
```

Rules:

- `parts` is rendered in order; later parts do not hide earlier ones unless geometry overlaps.
- `shape`: `box`, `sphere`, `cylinder`, `cone`, `torus`, `plane`.
- `position`, `rotation` (degrees), `scale` are `[x, y, z]`.
- `color` is `#rrggbb`; alpha is not supported.
- `segments` (8–32) controls curved shapes. Low values look intentionally faceted.
- The output is one mesh with `COLOR_0`; materials are white PBR so vertex colours show.
- Paths are workspace-relative and resolved through the workspace allowlist; parent folders
  are created.

## Recipes

- **Character:** box torso + box head + two box legs + two box arms; a sphere for the head
  reads better at small sizes. Use one accent colour for the face or chest.
- **Tree:** cylinder trunk (`#5d3d30`) + cone canopy (`#3f5d3a`), two overlapping cones for
  a fuller silhouette.
- **Rock:** sphere with a low segment count (8–10) and a non-uniform scale; a second smaller
  sphere offset to break the symmetry.
- **Terrain tile:** a flat box (top at `y = 0`) plus a few boxes for ledges; keep the walkable
  surface flat so collision stays simple.
- **Weapon/prop:** cylinder or box primitives along one axis; keep the grip at the origin.

## Loading in Three.js

```js
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';
const loader = new GLTFLoader();
const gltf = await loader.loadAsync(new URL('./assets/models/hero.gltf', import.meta.url));
scene.add(gltf.scene);
```

- The glTF is a single mesh; traverse it only if you need to change the material.
- Set `mesh.castShadow = true` and `receiveShadow = true` when the scene has shadows.
- Scale the loaded scene only if the game's unit scale differs; do not rescale per frame.
- If you need several copies, `SkeletonUtils.clone` is unnecessary for static meshes —
  `gltf.scene.clone(true)` is enough.

## Do not

- Do not reference external textures or remote assets; the tool cannot fetch them and the
  preview cannot rely on them.
- Do not emit OBJ/FBX by hand; use the tool so the file is valid glTF with correct accessors.
- Do not create one tiny model per box when a single `parts` array expresses the whole prop.
