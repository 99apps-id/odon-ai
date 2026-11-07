import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { RoomEnvironment } from 'three/examples/jsm/environments/RoomEnvironment.js';
import { installWebGLPrecisionPolyfill } from '../../lib/webglPolyfill';
import {
  ACTIVE_STATES,
  AREA_SUBAGENT,
  STATE_COLORS,
  areaFor,
  type Desk,
  type DeskState,
} from './model';
import {
  PALETTE,
  box,
  createDesk,
  createDirectorDesk,
  createDispenser,
  createGlassPartition,
  createLowWall,
  createMeetingTable,
  createPicture,
  createPlant,
  createSittingFigure,
  createSofa,
  createStandingPerson,
  createWhiteboard,
  createWindowWall,
  cylinder,
  disposeScreenTexture,
  makeLabel,
  type DeskVisual,
  type Person,
} from './furniture';

export interface OfficeScene {
  sync(desks: Desk[]): void;
  select(id: string): void;
  focus(id: string): void;
  reset(): void;
  /** Force the office lighting: 'auto' follows the clock, 'day'/'night' pin it. */
  setLightMode(mode: 'auto' | 'day' | 'night'): 'auto' | 'day' | 'night';
  /** Render one frame and return the canvas data URL for a PNG snapshot. */
  snapshot(): string;
  dispose(): void;
}

export const DIRECTOR_ID = 'director';
export const DIRECTOR_POSITION = new THREE.Vector3(0, 0.05, -13);

type Entry = DeskVisual & {
  id: string;
  state: DeskState;
  phase: number;
  sleepSprite: THREE.Sprite;
  thoughtBubble: THREE.Sprite;
  thoughtBubbleUntil: number;
  lastBubbleKind: 'thinking' | 'approval' | 'tool' | null;
};

const REDUCE_MOTION =
  typeof window !== 'undefined' &&
  window.matchMedia &&
  window.matchMedia('(prefers-reduced-motion: reduce)').matches;

const AGENT_SLOTS: Array<[number, number]> = [];
for (const z of [-4.5, 0, 4.5, -7.2, 7.2]) {
  for (const x of [-12.5, -4.2, 4.2, 12.5]) AGENT_SLOTS.push([x, z]);
}

const SUB_SLOTS: Array<[number, number]> = [];
for (const z of [10.2, 14.2, 5.6, 17.0]) {
  for (const x of [-12.5, -4.2, 4.2, 12.5]) SUB_SLOTS.push([x, z]);
}

const AISLES = [-8.35, 0, 8.35];
const FLOOR_Y = 0.05;

function slotOffset(area: string, slot: number): [number, number] {
  // Once the grid fills, spill into a shallow extension rather than stacking
  // a new desk exactly on top of an occupied one.
  if (area === AREA_SUBAGENT) {
    const base = SUB_SLOTS[slot % SUB_SLOTS.length];
    const row = Math.floor(slot / SUB_SLOTS.length);
    return [base[0], base[1] + row * 2.4];
  }
  const base = AGENT_SLOTS[slot % AGENT_SLOTS.length];
  const row = Math.floor(slot / AGENT_SLOTS.length);
  return [base[0], base[1] + row * 2.9];
}
const IDLE_SHIRTS = [
  '#6f7f8f',
  '#8d6e63',
  '#7c8f74',
  '#a8775f',
  '#8d8b7a',
  '#5f7f8f',
  '#9b7f6a',
];

/** Idle and completed agents keep their agent colour; active states stay colour-coded. */
function shirtFor(desk: Desk): string {
  if (ACTIVE_STATES.has(desk.state)) {
    return STATE_COLORS[desk.state];
  }
  if (desk.color) return desk.color;
  let hash = 0;
  for (let index = 0; index < desk.id.length; index += 1) {
    hash = (hash * 31 + desk.id.charCodeAt(index)) >>> 0;
  }
  return IDLE_SHIRTS[hash % IDLE_SHIRTS.length];
}

interface Walker {
  deskId: string;
  group: THREE.Group;
  person: Person;
  cup: THREE.Mesh;
  waypoints: THREE.Vector3[];
  index: number;
  returning: boolean;
  waitUntil: number;
  speed: number;
  /** How long the walker pauses at the destination before returning. */
  waitMs: number;
  /** A stroll while resting, or a status report to the director's office. */
  reason: 'rest' | 'report';
}

interface Pacer {
  group: THREE.Group;
  person: Person;
  route: THREE.Vector3[];
  index: number;
  waitUntil: number;
  speed: number;
  phase: number;
}

interface ThoughtBubble {
  sprite: THREE.Sprite;
  material: THREE.SpriteMaterial;
  kind: 'thinking' | 'approval' | 'tool';
  until: number;
}

function bubbleText(kind: 'thinking' | 'approval' | 'tool'): string {
  if (kind === 'thinking') return '💭';
  if (kind === 'approval') return '⚠️';
  return '🛠️';
}

function makeThoughtBubble(kind: 'thinking' | 'approval' | 'tool'): ThoughtBubble {
  const canvas = document.createElement('canvas');
  canvas.width = 256;
  canvas.height = 256;
  const ctx = canvas.getContext('2d');
  if (ctx) {
    // Speech bubble with a small tail.
    ctx.fillStyle = 'rgba(255, 250, 242, 0.96)';
    ctx.strokeStyle = 'rgba(214, 207, 195, 0.95)';
    ctx.lineWidth = 6;
    ctx.beginPath();
    if (ctx.roundRect) ctx.roundRect(18, 18, 220, 160, 38);
    else ctx.rect(18, 18, 220, 160);
    ctx.fill();
    ctx.stroke();
    ctx.beginPath();
    ctx.moveTo(98, 178);
    ctx.lineTo(128, 222);
    ctx.lineTo(150, 176);
    ctx.closePath();
    ctx.fill();
    ctx.stroke();
    ctx.font = '120px system-ui, "Segoe UI", sans-serif';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.fillText(bubbleText(kind), 128, 102);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.anisotropy = 4;
  const material = new THREE.SpriteMaterial({
    map: texture,
    transparent: true,
    depthTest: false, // draw above walls so the bubble is always visible
    opacity: 0,
  });
  const sprite = new THREE.Sprite(material);
  sprite.scale.set(0.5, 0.5, 1);
  sprite.position.set(0, 2.05, 0);
  sprite.visible = false;
  return { sprite, material, kind, until: 0 };
}

function makeFloorTexture(): THREE.CanvasTexture {
  const canvas = document.createElement('canvas');
  canvas.width = 1024;
  canvas.height = 1024;
  const ctx = canvas.getContext('2d');
  if (ctx) {
    // Office ceramic / porcelain tile layout:
    // 8x8 tiles per texture block (128x128px per tile) with distinct grout lines.
    const tileSize = 128;
    const groutSize = 2.5;

    // Grout seam background: subtle warm neutral grout
    ctx.fillStyle = '#948a7b';
    ctx.fillRect(0, 0, 1024, 1024);

    let seed = 42891;
    const rand = () => {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      return seed / 0xffffffff;
    };

    // Soft palette of office porcelain / ceramic tiles:
    // Muted architectural stone/taupe tones (calm, warm neutral, non-glaring)
    const tileColors = [
      '#bfb8ad',
      '#bbb3a8',
      '#b6afa4',
      '#c2bbb0',
      '#b3aca1',
      '#b8b1a6',
      '#bdb6ab',
      '#b1aaa0',
    ];

    for (let r = 0; r < 8; r++) {
      for (let c = 0; c < 8; c++) {
        const x = c * tileSize;
        const y = r * tileSize;
        const color = tileColors[Math.floor(rand() * tileColors.length)];

        // Tile body (inside the grout seam)
        ctx.fillStyle = color;
        ctx.fillRect(x + groutSize, y + groutSize, tileSize - groutSize * 2, tileSize - groutSize * 2);

        // Subtle bevel on tile edges (gives realistic tile depth without specular glare)
        // Top & left bevel highlight (gentle soft light)
        ctx.fillStyle = 'rgba(255, 255, 255, 0.09)';
        ctx.fillRect(x + groutSize, y + groutSize, tileSize - groutSize * 2, 2);
        ctx.fillRect(x + groutSize, y + groutSize, 2, tileSize - groutSize * 2);

        // Bottom & right bevel shadow (gentle recessed shade)
        ctx.fillStyle = 'rgba(50, 42, 35, 0.12)';
        ctx.fillRect(x + groutSize, y + tileSize - groutSize - 2, tileSize - groutSize * 2, 2);
        ctx.fillRect(x + tileSize - groutSize - 2, y + groutSize, 2, tileSize - groutSize * 2);

        // Subtle stone / ceramic matte grain on each tile
        for (let s = 0; s < 45; s++) {
          const sx = x + groutSize + rand() * (tileSize - groutSize * 2 - 2);
          const sy = y + groutSize + rand() * (tileSize - groutSize * 2 - 2);
          const opacity = (rand() * 0.04 + 0.015).toFixed(3);
          ctx.fillStyle = rand() > 0.5 ? `rgba(255, 255, 255, ${opacity})` : `rgba(60, 50, 40, ${opacity})`;
          ctx.fillRect(sx, sy, 2, 2);
        }
      }
    }
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.anisotropy = 4;
  texture.wrapS = THREE.RepeatWrapping;
  texture.wrapT = THREE.RepeatWrapping;
  // Repeat 6x5 across the 48x42 meter office floor so tiles are proportional to desks
  texture.repeat.set(6, 5);
  return texture;
}

/**
 * A soft radial pool directly under each pendant, so the brightest area sits
 * beneath the bulb rather than sideways from it.
 */
function makeLightPoolTexture(): THREE.CanvasTexture {
  const canvas = document.createElement('canvas');
  canvas.width = 256;
  canvas.height = 256;
  const ctx = canvas.getContext('2d');
  if (ctx) {
    const gradient = ctx.createRadialGradient(128, 128, 0, 128, 128, 128);
    gradient.addColorStop(0, 'rgba(255, 225, 160, 0.52)');
    gradient.addColorStop(0.45, 'rgba(255, 215, 140, 0.24)');
    gradient.addColorStop(1, 'rgba(255, 210, 130, 0)');
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, 256, 256);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.anisotropy = 2;
  return texture;
}

function disposeSpritePair(sprite: THREE.Sprite) {
  const material = sprite.material as THREE.SpriteMaterial;
  material.map?.dispose();
  material.dispose();
}

export function makeSkyTexture(night = false): THREE.CanvasTexture {
  const canvas = document.createElement('canvas');
  canvas.width = 1024;
  canvas.height = 512;
  const ctx = canvas.getContext('2d');
  if (ctx) {
    const gradient = ctx.createLinearGradient(0, 0, 0, 512);
    if (night) {
      gradient.addColorStop(0, '#0e1a2c');
      gradient.addColorStop(0.55, '#26344a');
      gradient.addColorStop(1, '#3a4157');
    } else {
      gradient.addColorStop(0, '#7fb2cf');
      gradient.addColorStop(0.55, '#cfe4ee');
      gradient.addColorStop(1, '#eef3f1');
    }
    ctx.fillStyle = gradient;
    ctx.fillRect(0, 0, 1024, 512);

    if (night) {
      for (let star = 0; star < 90; star += 1) {
        ctx.fillStyle = 'rgba(255,255,255,0.85)';
        const x = (star * 97) % 1024;
        const y = (star * 53) % 360;
        ctx.fillRect(x, y, 1.6, 1.6);
      }
    }

    ctx.fillStyle = night ? 'rgba(38, 51, 70, 0.52)' : 'rgba(255, 255, 255, 0.52)';
    for (const [x, y, scale] of [
      [170, 120, 1.2],
      [430, 72, 0.8],
      [740, 142, 1.4],
      [900, 64, 0.65],
    ] as const) {
      ctx.beginPath();
      ctx.ellipse(x, y, 86 * scale, 24 * scale, 0, 0, Math.PI * 2);
      ctx.ellipse(x + 48 * scale, y - 13 * scale, 52 * scale, 20 * scale, 0, 0, Math.PI * 2);
      ctx.fill();
    }

    // A soft skyline so the windows read as a city view, not a void.
    ctx.fillStyle = night ? '#1d2a3a' : '#b6c8cf';
    for (let index = 0; index < 22; index += 1) {
      const width = 18 + (index % 5) * 12;
      const height = 24 + ((index * 37) % 82);
      ctx.fillRect(index * 48, 512 - height, width, height);
    }
    ctx.fillStyle = night ? '#16242f' : '#a4b8c0';
    for (let index = 0; index < 16; index += 1) {
      const width = 24 + (index % 4) * 16;
      const height = 14 + ((index * 53) % 46);
      ctx.fillRect(12 + index * 66, 512 - height, width, height);
    }
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.anisotropy = 4;
  return texture;
}

function animateStanding(person: Person, now: number, moving: boolean) {
  const swing = moving ? Math.sin(now * 0.012) * 0.55 : 0;
  person.leftLeg.rotation.x = swing;
  person.rightLeg.rotation.x = -swing;
  person.leftArm.rotation.x = 0.12 - swing * 0.7;
  person.rightArm.rotation.x = 0.12 + swing * 0.7;
}

function disposeObject(root: THREE.Object3D) {
  root.traverse((node) => {
    if (node instanceof THREE.Mesh) {
      node.geometry.dispose();
      const material = node.material;
      if (Array.isArray(material)) material.forEach((entry) => entry.dispose());
      else material.dispose();
    } else if (node instanceof THREE.Sprite) {
      (node.material as THREE.SpriteMaterial).map?.dispose();
      node.material.dispose();
    }
  });
}

/** Floating "z z z" that marks an idle agent as asleep. */
function makeSleepSprite(): THREE.Sprite {
  const canvas = document.createElement('canvas');
  canvas.width = 128;
  canvas.height = 128;
  const ctx = canvas.getContext('2d');
  if (ctx) {
    ctx.clearRect(0, 0, 128, 128);
    ctx.fillStyle = '#6b7280';
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    ctx.font = '700 54px system-ui, "Segoe UI", sans-serif';
    ctx.fillText('z', 42, 78);
    ctx.font = '700 38px system-ui, "Segoe UI", sans-serif';
    ctx.fillText('z', 78, 50);
    ctx.font = '700 26px system-ui, "Segoe UI", sans-serif';
    ctx.fillText('z', 104, 28);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  const sprite = new THREE.Sprite(
    new THREE.SpriteMaterial({ map: texture, transparent: true, depthWrite: false }),
  );
  sprite.scale.set(0.72, 0.72, 1);
  sprite.visible = false;
  return sprite;
}

/**
 * Build the Three.js office. Throws when WebGL is unavailable; the view then
 * falls back to a plain grid so the data is still usable.
 */
export function createOfficeScene(
  canvas: HTMLCanvasElement,
  onSelect: (id: string) => void,
  onHover?: (id: string, clientX: number, clientY: number) => void,
): OfficeScene {
  installWebGLPrecisionPolyfill();

  let renderer: THREE.WebGLRenderer;
  try {
    renderer = new THREE.WebGLRenderer({
      canvas,
      antialias: true,
      powerPreference: 'default',
    });
  } catch (err) {
    console.warn('[ODON 3D] Failed to create antialiased WebGLRenderer, retrying with fallback:', err);
    renderer = new THREE.WebGLRenderer({
      canvas,
      antialias: false,
      precision: 'mediump',
      powerPreference: 'default',
    });
  }

  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 0.96;
  renderer.outputColorSpace = THREE.SRGBColorSpace;

  const scene = new THREE.Scene();
  scene.background = new THREE.Color('#dcd6cb');

  // Image-based lighting gives the flat primitives a believable ambient
  // bounce so whites are not blown and wood/plastic read as materials.
  try {
    const pmrem = new THREE.PMREMGenerator(renderer);
    const environment = new RoomEnvironment();
    scene.environment = pmrem.fromScene(environment, 0.04).texture;
    environment.dispose();
    pmrem.dispose();
    if ('environmentIntensity' in scene) {
      (scene as any).environmentIntensity = 0.25;
    }
  } catch (envErr) {
    console.warn('[ODON 3D] RoomEnvironment PMREM skipped (unsupported by GPU/WebGL context):', envErr);
  }

  const camera = new THREE.OrthographicCamera(-22, 22, 22, -22, 0.1, 260);
  camera.position.set(31, 29, 37);

  const controls = new OrbitControls(camera, canvas);
  controls.target.set(0, 1, -1);
  controls.enablePan = true;
  controls.screenSpacePanning = true;
  controls.enableDamping = true;
  controls.dampingFactor = 0.08;
  controls.minZoom = 0.6;
  controls.maxZoom = 9;
  controls.zoomSpeed = 0.9;
  controls.minPolarAngle = 0.5;
  controls.maxPolarAngle = Math.PI / 2.3;
  controls.addEventListener('change', () => {
    controls.target.x = THREE.MathUtils.clamp(controls.target.x, -19, 19);
    controls.target.y = THREE.MathUtils.clamp(controls.target.y, 0.4, 3);
    controls.target.z = THREE.MathUtils.clamp(controls.target.z, -16, 15);
  });
  controls.update();

  const hemi = new THREE.HemisphereLight('#f2ebdc', '#726b60', 0.8);
  scene.add(hemi);
  const sun = new THREE.DirectionalLight('#fff0d8', 1.35);
  sun.position.set(19, 29, 12);
  sun.castShadow = true;
  sun.shadow.mapSize.set(2048, 2048);
  sun.shadow.camera.left = -31;
  sun.shadow.camera.right = 31;
  sun.shadow.camera.top = 31;
  sun.shadow.camera.bottom = -31;
  sun.shadow.camera.near = 1;
  sun.shadow.camera.far = 100;
  sun.shadow.bias = -0.0004;
  sun.shadow.normalBias = 0.02;
  scene.add(sun);
  const fill = new THREE.DirectionalLight('#dce7ed', 0.35);
  fill.position.set(-16, 18, -12);
  scene.add(fill);

  // Ceiling pendants across the whole floor; their bulbs glow as it gets dark.
  const lampZones: Array<{ x: number; z: number; y: number; on: number }> = [];
  for (const z of [-10.6, -3.5, 3.5, 9.6]) {
    for (const x of [-13.5, -4.5, 4.5, 13.5]) {
      lampZones.push({ x, z, y: 3.15, on: 0 });
    }
  }
  for (const [x, z, y] of [
    [0, -13, 3.0],
    [-15, -13, 2.9],
    [15, -13, 2.9],
    [0, 12.4, 3.15],
  ] as const) {
    lampZones.push({ x, z, y, on: 0 });
  }

  const bulbMaterials: THREE.MeshBasicMaterial[] = [];
  const bulbMeshes: THREE.Mesh[] = [];
  const sconceMaterials: THREE.MeshBasicMaterial[] = [];
  const lampLights: THREE.PointLight[] = [];
  const ceilingLights: THREE.PointLight[] = [];
  const deskGlowMaterials: THREE.MeshBasicMaterial[] = [];
  for (const zone of lampZones) {
    // Pendant: rod, shade, glowing bulb. No light here; the real (few)
    // point lights are placed below, one per room.
    const rod = cylinder(0.02, 0.02, 0.35, '#5d3d30', 8);
    rod.position.set(zone.x, zone.y + 0.36, zone.z);
    scene.add(rod);
    const shade = new THREE.Mesh(
      new THREE.ConeGeometry(0.26, 0.22, 16, 1, true),
      new THREE.MeshStandardMaterial({
        color: '#5d3d30',
        roughness: 0.7,
        side: THREE.DoubleSide,
      }),
    );
    shade.position.set(zone.x, zone.y + 0.22, zone.z);
    scene.add(shade);
    const bulbMaterial = new THREE.MeshBasicMaterial({ color: '#fff0c8' });
    const bulb = new THREE.Mesh(new THREE.SphereGeometry(0.055, 10, 8), bulbMaterial);
    bulb.position.set(zone.x, zone.y + 0.16, zone.z);
    scene.add(bulb);
    bulbMaterials.push(bulbMaterial);
    bulbMeshes.push(bulb);
  }
  // Real room lights: warm, low intensity, one per named room + aisle pools.
  for (const [x, z] of [
    [0, -13],
    [-15, -13],
    [15, -13],
    [0, -1],
    [0, 11.8],
  ] as const) {
    const light = new THREE.PointLight('#ffd9a0', 0, 15, 1.8);
    light.position.set(x, 2.9, z);
    scene.add(light);
    lampLights.push(light);
  }
  // Sconce lights in the director's glass/ meeting room/ lounge/ subagent wing.
  for (const [x, y, z] of [
    [-2.5, 2.2, -8.3],
    [2.5, 2.2, -8.3],
    [-18.5, 2.05, -17.15],
    [18.5, 2.05, -17.15],
    [0, 1.6, 7.1],
  ] as const) {
    const sconce = new THREE.Mesh(
      new THREE.SphereGeometry(0.09, 10, 8),
      new THREE.MeshBasicMaterial({ color: '#ffe6b8' }),
    );
    sconce.position.set(x, y, z);
    scene.add(sconce);
    sconceMaterials.push(sconce.material as THREE.MeshBasicMaterial);
    const rod = cylinder(0.015, 0.015, 0.3, '#5d3d30', 6);
    rod.position.set(x, y + 0.2, z);
    scene.add(rod);
  }
  // Soft pools over the desks so monitors read at night. These sit exactly
  // under four of the pendants, matching the floor pools below.
  for (const [x, z] of [
    [-4.5, -3.5],
    [4.5, -3.5],
    [-4.5, 3.5],
    [4.5, 3.5],
    [-4.5, 9.6],
    [4.5, 9.6],
  ] as const) {
    const light = new THREE.PointLight('#cfe2ee', 0, 7, 2.0);
    light.position.set(x, 1.9, z);
    scene.add(light);
    ceilingLights.push(light);
  }

  // A sky backdrop behind the two window walls.
  const skyTexture = makeSkyTexture();
  const skyNightTexture = makeSkyTexture(true);
  const skyMaterial = new THREE.MeshBasicMaterial({ map: skyTexture, toneMapped: false });
  const skyBack = new THREE.Mesh(new THREE.PlaneGeometry(70, 20), skyMaterial);
  skyBack.position.set(0, 8, -24.2);
  scene.add(skyBack);
  const skyLeft = new THREE.Mesh(new THREE.PlaneGeometry(55, 20), skyMaterial);
  skyLeft.rotation.y = Math.PI / 2;
  skyLeft.position.set(-26, 8, -1);
  scene.add(skyLeft);

  // Base slab and floor.
  const slab = box(49, 0.4, 43, '#8e8678', 0, -0.21, -1);
  slab.castShadow = false;
  slab.receiveShadow = true;
  scene.add(slab);
  // An office ceramic / porcelain tile canvas floor texture gives the office
  // a realistic, anti-glare architectural tile ground.
  const floorTexture = makeFloorTexture();
  const floor = new THREE.Mesh(
    new THREE.PlaneGeometry(48, 42),
    new THREE.MeshStandardMaterial({
      map: floorTexture,
      color: '#ded8cc',
      roughness: 0.95,
      metalness: 0.02,
    }),
  );
  floor.rotation.x = -Math.PI / 2;
  floor.position.set(0, 0, -1);
  floor.receiveShadow = true;
  scene.add(floor);
  const grid = new THREE.GridHelper(48, 12, '#9c9284', '#bbb1a2');
  grid.position.set(0, 0.006, -1);
  (grid.material as THREE.Material).opacity = 0.06;
  (grid.material as THREE.Material).transparent = true;
  scene.add(grid);

  // Light pools for each pendant, projected onto the floor directly below
  // the bulb. One shared texture is used for all of them so we can keep them
  // aligned with the fixtures. They glow only as darkness rises.
  const poolTexture = makeLightPoolTexture();
  const poolMaterial = new THREE.MeshBasicMaterial({
    map: poolTexture,
    transparent: true,
    opacity: 0,
    blending: THREE.AdditiveBlending,
    depthWrite: false,
    toneMapped: false,
  });
  const pools: THREE.Mesh[] = [];
  for (const zone of lampZones) {
    const pool = new THREE.Mesh(new THREE.PlaneGeometry(6.6, 6.6), poolMaterial);
    pool.rotation.x = -Math.PI / 2;
    pool.position.set(zone.x, 0.035, zone.z);
    pool.renderOrder = 2;
    scene.add(pool);
    pools.push(pool);
  }
  // The night glow is strongest under the pendant: the cool blue monitor pools
  // were nudging the same desks sideways, so they now share the pendant grid.
  for (const light of lampLights) {
    light.position.y -= 0.15;
  }

  function addRug(width: number, depth: number, x: number, z: number, color: string) {
    const border = box(width + 0.3, 0.02, depth + 0.3, '#cec2b0', x, 0.008, z);
    border.castShadow = false;
    border.receiveShadow = true;
    scene.add(border);
    const rug = box(width, 0.04, depth, color, x, 0.025, z);
    rug.castShadow = false;
    rug.receiveShadow = true;
    scene.add(rug);
  }
  addRug(38.5, 13, 0, -0.5, '#d3e2d3');
  addRug(38.5, 10.5, 0, 11.2, '#d2e2ea');
  addRug(12.2, 9.2, 0, -13, '#efd7c2');
  addRug(12.0, 9.2, -15, -13, '#dbe6d5');
  addRug(12.0, 9.2, 15, -13, '#f1d6c0');

  function addCredenza(
    width: number,
    x: number,
    z: number,
    color: string,
    rotationY = 0,
  ) {
    const group = new THREE.Group();
    group.add(box(width, 0.78, 0.42, color, 0, 0.39, 0));
    group.add(box(width + 0.06, 0.05, 0.46, '#b7ab99', 0, 0.805, 0));
    group.add(box(width - 0.16, 0.12, 0.03, '#5d3d30', 0, 0.2, 0.215));
    group.add(box(0.05, 0.26, 0.05, '#5d3d30', -width / 2 + 0.14, 0.13, 0.21));
    group.add(box(0.05, 0.26, 0.05, '#5d3d30', width / 2 - 0.14, 0.13, 0.21));
    group.position.set(x, FLOOR_Y, z);
    group.rotation.y = rotationY;
    scene.add(group);
    return group;
  }

  // Shell: two window walls give the office a real outside.
  const backWall = createWindowWall(45);
  backWall.position.set(0, 0, -20.2);
  scene.add(backWall);
  const leftWall = createWindowWall(40);
  leftWall.rotation.y = Math.PI / 2;
  leftWall.position.set(-23, 0, -1);
  scene.add(leftWall);
  scene.add(box(45, 0.16, 0.2, '#cfc3b0', 0, 0.08, -19.98));
  scene.add(box(0.2, 0.16, 40, '#cfc3b0', -22.78, 0.08, -1));

  const labels: THREE.Sprite[] = [];
  function addLabel(text: string, x: number, y: number, z: number, size = 0.58) {
    const label = makeLabel(text, size);
    label.position.set(x, y, z);
    label.visible = true;
    scene.add(label);
    labels.push(label);
    return label;
  }

  // Director's office: glass walls and the back window behind the boss.
  const directorGlassLeft = createGlassPartition(5.4, 2.0);
  directorGlassLeft.position.set(-3.5, 0, -8.2);
  scene.add(directorGlassLeft);
  const directorGlassRight = createGlassPartition(5.0, 2.0);
  directorGlassRight.position.set(3.7, 0, -8.2);
  scene.add(directorGlassRight);
  const directorSideLeft = createGlassPartition(9.4, 2.0);
  directorSideLeft.rotation.y = Math.PI / 2;
  directorSideLeft.position.set(-6.2, 0, -12.9);
  scene.add(directorSideLeft);
  const directorSideRight = createGlassPartition(9.4, 2.0);
  directorSideRight.rotation.y = Math.PI / 2;
  directorSideRight.position.set(6.2, 0, -12.9);
  scene.add(directorSideRight);
  scene.add(box(2.1, 0.1, 0.12, '#b7ab99', 0.2, 2.06, -8.2));
  scene.add(box(0.1, 2.0, 0.12, '#b7ab99', -0.85, 1.0, -8.2));
  scene.add(box(0.1, 2.0, 0.12, '#b7ab99', 1.25, 1.0, -8.2));

  addCredenza(2.6, 0, -17.15, '#9fb3bf');
  const portraitLeft = createPicture('#557a95');
  portraitLeft.position.set(-0.75, 1.2, -16.95);
  portraitLeft.rotation.x = -0.1;
  scene.add(portraitLeft);
  const portraitRight = createPicture('#a8775f');
  portraitRight.position.set(0.75, 1.2, -16.95);
  portraitRight.rotation.x = -0.1;
  scene.add(portraitRight);

  const directorDesk = createDirectorDesk();
  directorDesk.position.copy(DIRECTOR_POSITION);
  directorDesk.userData.sessionId = DIRECTOR_ID;
  scene.add(directorDesk);

  const bookshelf = new THREE.Group();
  bookshelf.add(box(1.7, 2.1, 0.42, '#d9cbb8', 0, 1.05, 0));
  for (const y of [0.35, 0.8, 1.25, 1.7]) {
    bookshelf.add(box(1.56, 0.05, 0.38, '#b7ab99', 0, y, 0.03));
  }
  const bookColors = ['#557a95', '#a8775f', '#5f7d58', '#d8b45a', '#6b4a2f', '#8d8f88'];
  for (let shelf = 0; shelf < 4; shelf += 1) {
    for (let book = 0; book < 6; book += 1) {
      if ((shelf + book) % 5 === 0) continue;
      const height = 0.22 + ((shelf * 7 + book * 3) % 4) * 0.035;
      bookshelf.add(
        box(
          0.085,
          height,
          0.22,
          bookColors[(shelf + book) % bookColors.length],
          -0.6 + book * 0.2,
          0.4 + shelf * 0.45 + height / 2,
          0.07,
        ),
      );
    }
  }
  bookshelf.position.set(5.25, FLOOR_Y, -11.4);
  scene.add(bookshelf);

  const directorPlant = createPlant(1.25);
  directorPlant.position.set(4.7, FLOOR_Y, -16.4);
  scene.add(directorPlant);
  addLabel("DIRECTOR'S OFFICE", 0, 3.5, -12.6, 1.0);

  // Meeting room: low walls, whiteboard, table and a clock.
  scene.add(box(11.2, 2.4, 0.16, '#b9c7b5', -15, 1.2, -17.35));
  const meetingSide = createLowWall(9.4);
  meetingSide.rotation.y = Math.PI / 2;
  meetingSide.position.set(-8.8, 0, -12.9);
  scene.add(meetingSide);
  const meetingFront = createLowWall(11.0);
  meetingFront.position.set(-15.7, 0, -8.2);
  scene.add(meetingFront);

  const whiteboard = createWhiteboard();
  whiteboard.position.set(-15, FLOOR_Y, -15.7);
  scene.add(whiteboard);
  const meetingTable = createMeetingTable();
  meetingTable.position.set(-15, FLOOR_Y, -12.7);
  scene.add(meetingTable);

  // Two people sit at the table, facing each other, while the side chairs
  // stay empty.
  const meetingSitter1 = createSittingFigure('#6f7f8f', [-1.1, 0, 1.05], Math.PI);
  meetingSitter1.position.set(-1.1, FLOOR_Y + 0.04, -12.7 + 1.05);
  scene.add(meetingSitter1);
  const meetingSitter2 = createSittingFigure('#7c8f74', [0.2, 0, -1.05], 0);
  meetingSitter2.position.set(0.2, FLOOR_Y + 0.04, -12.7 - 1.05);
  scene.add(meetingSitter2);

  const meetingFrame = createPicture('#3f5d3a');
  meetingFrame.position.set(-11.4, 1.65, -17.24);
  scene.add(meetingFrame);
  scene.add(box(1.2, 0.8, 0.03, '#8fa9b5', -18.6, 1.35, -17.24));
  scene.add(box(1.2, 0.8, 0.03, '#c98f73', -10.3, 1.35, -17.24));
  const meetingFrontArt = createPicture('#557a95');
  meetingFrontArt.position.set(-18.2, 0.62, -8.06);
  scene.add(meetingFrontArt);
  const clock = new THREE.Group();
  const clockFace = cylinder(0.28, 0.28, 0.06, '#fffaf2', 24);
  clockFace.rotation.x = Math.PI / 2;
  clock.add(clockFace);
  const clockRim = new THREE.Mesh(
    new THREE.TorusGeometry(0.28, 0.025, 8, 24),
    new THREE.MeshStandardMaterial({ color: '#5d3d30', roughness: 0.8 }),
  );
  clockRim.position.z = 0.035;
  clock.add(clockRim);
  const clockHourHand = box(0.018, 0.17, 0.012, '#3a352e', 0, 0.075, 0.045);
  clock.add(clockHourHand);
  const clockMinuteHand = box(0.014, 0.12, 0.012, '#3a352e', 0.045, 0.02, 0.047);
  clockMinuteHand.rotation.z = -1.1;
  clock.add(clockMinuteHand);
  clock.position.set(-18.4, 2.05, -17.2);
  scene.add(clock);

  const meetingPlant = createPlant(1.1);
  meetingPlant.position.set(-20.1, FLOOR_Y, -16.4);
  scene.add(meetingPlant);
  addLabel('MEETING ROOM', -15, 3.35, -12.6, 0.9);

  // Lounge: dispenser, sofa, coffee table and a big window.
  const loungeSide = createLowWall(9.4);
  loungeSide.rotation.y = Math.PI / 2;
  loungeSide.position.set(8.8, 0, -12.9);
  scene.add(loungeSide);
  const loungeFrontLeft = createLowWall(1.4);
  loungeFrontLeft.position.set(9.5, 0, -8.2);
  scene.add(loungeFrontLeft);
  const loungeFrontRight = createLowWall(9.0);
  loungeFrontRight.position.set(16.7, 0, -8.2);
  scene.add(loungeFrontRight);

  const dispenser = createDispenser();
  dispenser.position.set(12.6, FLOOR_Y, -16.7);
  scene.add(dispenser);
  const sofa = createSofa();
  sofa.position.set(18.2, FLOOR_Y, -14.6);
  scene.add(sofa);
  const coffeeTable = new THREE.Group();
  coffeeTable.add(box(1.5, 0.1, 0.85, '#e5dccd', 0, 0.4, 0));
  for (const [x, z] of [
    [-0.62, -0.3],
    [0.62, -0.3],
    [-0.62, 0.3],
    [0.62, 0.3],
  ] as const) {
    coffeeTable.add(box(0.07, 0.38, 0.07, '#5d3d30', x, 0.2, z));
  }
  coffeeTable.add(box(0.28, 0.03, 0.2, PALETTE.white, 0.1, 0.45, 0.05));
  coffeeTable.position.set(18.2, FLOOR_Y, -12.7);
  scene.add(coffeeTable);

  addCredenza(1.9, 17.3, -17.15, '#c98f73');
  const loungeFrame = createPicture('#557a95');
  loungeFrame.position.set(17.3, 1.2, -16.95);
  loungeFrame.rotation.x = -0.1;
  scene.add(loungeFrame);
  const loungeFrontArt = createPicture('#c98f73');
  loungeFrontArt.position.set(15.8, 0.62, -8.06);
  scene.add(loungeFrontArt);
  const consolePlant = createPlant(0.5);
  consolePlant.position.set(18.2, FLOOR_Y + 0.86, -17.15);
  scene.add(consolePlant);
  const lamp = new THREE.Group();
  lamp.add(cylinder(0.18, 0.2, 0.05, '#2b2721', 12));
  const lampPole = cylinder(0.022, 0.022, 1.5, '#5d3d30', 8);
  lampPole.position.y = 0.77;
  lamp.add(lampPole);
  const lampShade = new THREE.Mesh(
    new THREE.ConeGeometry(0.28, 0.34, 16, 1, true),
    new THREE.MeshStandardMaterial({
      color: '#fff3df',
      emissive: '#ffdca8',
      emissiveIntensity: 0.4,
      roughness: 0.85,
      side: THREE.DoubleSide,
    }),
  );
  lampShade.position.y = 1.58;
  lamp.add(lampShade);
  lamp.position.set(15.2, FLOOR_Y, -15.2);
  scene.add(lamp);
  const loungePlant = createPlant(1.2);
  loungePlant.position.set(20.2, FLOOR_Y, -16.4);
  scene.add(loungePlant);
  const loungePlantSmall = createPlant(0.85);
  loungePlantSmall.position.set(9.6, FLOOR_Y, -16.3);
  scene.add(loungePlantSmall);
  addLabel('LOUNGE', 15, 3.35, -12.6, 0.9);

  // Subagent wing behind a low glass partition.
  const subPartitionLeft = createGlassPartition(19.4, 1.15);
  subPartitionLeft.position.set(-11.3, 0, 6.8);
  scene.add(subPartitionLeft);
  const subPartitionRight = createGlassPartition(19.4, 1.15);
  subPartitionRight.position.set(11.3, 0, 6.8);
  scene.add(subPartitionRight);
  addLabel('SUBAGENT WING', 0, 3.5, 11.8, 1.2);

  const huddle = new THREE.Group();
  huddle.add(box(2.6, 0.1, 1.0, '#e5dccd', 0, 0.74, 0));
  for (const [x, z] of [
    [-1.1, -0.35],
    [1.1, -0.35],
    [-1.1, 0.35],
    [1.1, 0.35],
  ] as const) {
    huddle.add(box(0.08, 0.72, 0.08, '#5d3d30', x, 0.36, z));
  }
  huddle.add(box(0.38, 0.03, 0.26, '#2b2721', -0.4, 0.8, 0.05));
  huddle.add(box(0.24, 0.02, 0.18, PALETTE.white, 0.5, 0.8, 0.1));
  for (const [x, z, back] of [
    [-0.9, 0.95, 0.18],
    [0.4, 0.95, 0.18],
    [-0.9, -0.95, -0.18],
    [0.4, -0.95, -0.18],
  ] as const) {
    huddle.add(box(0.42, 0.08, 0.42, '#3a352e', x, 0.44, z));
    huddle.add(box(0.42, 0.45, 0.07, '#3a352e', x, 0.7, z + back));
  }
  const huddlePlant = createPlant(0.35);
  huddlePlant.position.set(1.0, 0.79, -0.12);
  huddle.add(huddlePlant);
  huddle.position.set(0, FLOOR_Y, 12.4);
  scene.add(huddle);
  const huddleSitter1 = createSittingFigure('#8d6e63', [-0.9, 0, 0.95], Math.PI);
  const huddleSitter2 = createSittingFigure('#8d8b7a', [0.4, 0, -0.95], 0);
  huddleSitter1.position.set(-0.9, FLOOR_Y + 0.04, 13.35);
  huddleSitter2.position.set(0.4, FLOOR_Y + 0.04, 11.45);
  scene.add(huddleSitter1, huddleSitter2);

  const agentPlantLeft = createPlant(1.35);
  agentPlantLeft.position.set(-20.3, FLOOR_Y, -6.1);
  scene.add(agentPlantLeft);
  const agentPlantRight = createPlant(1.35);
  agentPlantRight.position.set(20.3, FLOOR_Y, -6.1);
  scene.add(agentPlantRight);
  const subPlantLeft = createPlant(1.15);
  subPlantLeft.position.set(-20.4, FLOOR_Y, 16.3);
  scene.add(subPlantLeft);
  const subPlantRight = createPlant(1.15);
  subPlantRight.position.set(20.4, FLOOR_Y, 16.3);
  scene.add(subPlantRight);
  addLabel('AGENT FLOOR', 0, 3.55, -0.7, 1.25);

  addCredenza(2.4, -21.7, 1.0, '#b7c4c9', Math.PI / 2);
  const sideboardPlant = createPlant(0.6);
  sideboardPlant.position.set(-21.45, FLOOR_Y + 0.86, 1.9);
  scene.add(sideboardPlant);

  const sideShelf = new THREE.Group();
  sideShelf.add(box(1.6, 1.7, 0.4, '#d9cbb8', 0, 0.85, 0));
  for (const y of [0.42, 0.84, 1.26]) {
    sideShelf.add(box(1.48, 0.04, 0.36, '#b7ab99', 0, y, 0.02));
  }
  const shelfBooks = ['#557a95', '#a8775f', '#5f7d58', '#d8b45a', '#6b4a2f'];
  for (let index = 0; index < 10; index += 1) {
    const height = 0.2 + (index % 4) * 0.03;
    sideShelf.add(
      box(
        0.08,
        height,
        0.2,
        shelfBooks[index % shelfBooks.length],
        -0.52 + (index % 6) * 0.18,
        0.28 + Math.floor(index / 6) * 0.42 + height / 2,
        0.06,
      ),
    );
  }
  sideShelf.rotation.y = Math.PI / 2;
  sideShelf.position.set(20.4, FLOOR_Y, -1.4);
  scene.add(sideShelf);

  const printer = new THREE.Group();
  printer.add(box(0.9, 0.8, 0.65, '#e5dccd', 0, 0.4, 0));
  printer.add(box(0.7, 0.05, 0.3, '#b7ab99', 0, 0.84, 0.14));
  printer.add(box(0.22, 0.12, 0.03, '#2b3c49', 0, 0.6, 0.34));
  printer.add(box(0.3, 0.03, 0.22, PALETTE.white, 0.02, 0.86, 0.14));
  printer.position.set(19.6, FLOOR_Y, 2.2);
  scene.add(printer);

  for (const [x, z] of [
    [-3.0, 5.5],
    [10.8, 5.5],
  ] as const) {
    const bin = cylinder(0.16, 0.13, 0.4, '#5d6b72', 12);
    bin.position.set(x, FLOOR_Y + 0.2, z);
    scene.add(bin);
  }

  const entries = new Map<string, Entry>();
  const assignments = new Map<string, { area: string; slot: number }>();
  const pacers: Pacer[] = [];
  const MAX_WALKERS = 3;
  let walkers: Walker[] = [];
  // Fair rotation: the agents with the fewest completed walks go first, so
  // everyone takes a turn before anyone repeats.
  const walkCounts = new Map<string, number>();
  const lastReportAt = new Map<string, number>();
  let selectedId = '';
  let hoveredId = '';
  let nextWalkAt = performance.now() + 4000;

  function deskGlowColor(entry: Entry): string {
    if (entry.id === selectedId) return '#3f5b70';
    if (entry.id === hoveredId) return '#557a95';
    if (ACTIVE_STATES.has(entry.state)) return STATE_COLORS[entry.state];
    return '#b7b2a6';
  }

  function applySelection() {
    for (const entry of entries.values()) {
      const base = entry.small ? 0.78 : 1;
      const highlighted = entry.id === selectedId || entry.id === hoveredId;
      entry.group.scale.setScalar(base * (highlighted ? 1.08 : 1));
      entry.ringMaterial.color.set(deskGlowColor(entry));
    }
  }

  function removeEntry(id: string) {
    const entry = entries.get(id);
    if (!entry) return;
    cancelWalkFor(id, false);
    scene.remove(entry.group);
    disposeObject(entry.group);
    entries.delete(id);
    assignments.delete(id);
  }

  function makeEntry(desk: Desk): Entry {
    const small = areaFor(desk) === AREA_SUBAGENT;
    const built = createDesk(shirtFor(desk), small);
    built.ledMaterial.color.set(STATE_COLORS[desk.state]);
    built.ringMaterial.color.set(STATE_COLORS[desk.state]);
    built.group.userData.sessionId = desk.id;
    built.group.scale.setScalar(small ? 0.78 : 1);
    const sleepSprite = makeSleepSprite();
    sleepSprite.position.set(0.55, 2.05, 0.15);
    built.group.add(sleepSprite);
    const thoughtBubble = makeThoughtBubble('thinking').sprite;
    thoughtBubble.position.set(0, 2.02, 0);
    thoughtBubble.visible = false;
    built.group.add(thoughtBubble);
    const entry: Entry = {
      ...built,
      id: desk.id,
      state: desk.state,
      phase: Math.random() * Math.PI * 2,
      sleepSprite,
      thoughtBubble,
      thoughtBubbleUntil: 0,
      lastBubbleKind: null,
    };
    entries.set(desk.id, entry);
    scene.add(entry.group);
    return entry;
  }

  function cancelWalkFor(deskId: string, restore = true) {
    const index = walkers.findIndex((row) => row.deskId === deskId);
    if (index < 0) return;
    const [walker] = walkers.splice(index, 1);
    scene.remove(walker.group);
    disposeObject(walker.group);
    const entry = entries.get(deskId);
    if (restore && entry) entry.person.group.visible = true;
  }

  function cancelAllWalks(restore = false) {
    for (const walker of walkers) {
      scene.remove(walker.group);
      disposeObject(walker.group);
      if (restore) {
        const entry = entries.get(walker.deskId);
        if (entry) entry.person.group.visible = true;
      }
    }
    walkers = [];
  }

  /** Start one walker when a slot is free. Returns whether one started. */
  function startWalk(now: number): boolean {
    if (REDUCE_MOTION || walkers.length >= MAX_WALKERS) return false;
    const busy = new Set(walkers.map((walker) => walker.deskId));
    const eligible = [...entries.values()].filter(
      (entry) => !entry.small && entry.id !== selectedId && !busy.has(entry.id),
    );
    if (eligible.length === 0) return false;

    const active = eligible.filter((entry) => ACTIVE_STATES.has(entry.state));
    const resting = eligible.filter((entry) => !ACTIVE_STATES.has(entry.state));
    const reportable = active.filter(
      (entry) => now - (lastReportAt.get(entry.id) ?? 0) > 55_000,
    );

    let entry: Entry;
    let reason: 'rest' | 'report' = 'rest';
    // An active agent occasionally walks to the director's office to report,
    // pauses there, then returns to its desk and continues the task.
    if (reportable.length > 0 && Math.random() < 0.45) {
      reportable.sort(
        (a, b) => (lastReportAt.get(a.id) ?? 0) - (lastReportAt.get(b.id) ?? 0),
      );
      entry = reportable[0];
      reason = 'report';
    } else {
      // Fair rotation: fewest completed walks first, random tie-break.
      const pool = resting.length > 0 ? resting : eligible;
      const ranked = pool
        .map((row) => ({
          row,
          key: (walkCounts.get(row.id) ?? 0) + Math.random() * 0.25,
        }))
        .sort((a, b) => a.key - b.key);
      entry = ranked[0].row;
    }

    const person = createStandingPerson(
      '#' + entry.torsoMaterial.color.getHexString(),
    );
    const x = entry.group.position.x;
    const z = entry.group.position.z;
    const aisle = AISLES.reduce(
      (best, value) => (Math.abs(value - x) < Math.abs(best - x) ? value : best),
      AISLES[0],
    );
    person.group.position.set(x, FLOOR_Y, z + 1.45);
    person.group.rotation.y = 0;
    scene.add(person.group);

    const cup = cylinder(0.035, 0.03, 0.07, '#fffaf2', 10);
    cup.position.set(0.3, 0.86, 0.06);
    cup.visible = false;
    person.group.add(cup);

    const point = (px: number, pz: number) => new THREE.Vector3(px, FLOOR_Y, pz);
    const path = [
      point(x, z + 1.45),
      point(aisle, z + 1.45),
      point(aisle, 5.9),
      point(0, 5.9),
      point(0, -6.4),
    ];
    if (reason === 'report') {
      // Stop in front of the director's glass office instead of walking
      // through the partition.
      path.push(point(0, -9.4), point(0, -10.9));
    } else {
      // Walk through the lounge entrance (x=10.2, z=-8.2), then to the
      // dispenser. The entrance sits between the two low walls.
      path.push(
        point(10.2, -6.4),
        point(10.2, -8.2),
        point(10.2, -11.2),
        point(12.6, -11.2),
        point(12.6, -15.4),
      );
    }

    entry.person.group.visible = false;
    walkers.push({
      deskId: entry.id,
      group: person.group,
      person,
      cup,
      waypoints: path,
      index: 1,
      returning: false,
      waitUntil: 0,
      speed: 1.15 + Math.random() * 0.4,
      waitMs:
        reason === 'report'
          ? 2400 + Math.random() * 1600
          : 1200 + Math.random() * 1200,
      reason,
    });
    walkCounts.set(entry.id, (walkCounts.get(entry.id) ?? 0) + 1);
    if (reason === 'report') lastReportAt.set(entry.id, now);
    return true;
  }

  function updateWalkers(now: number, dt: number) {
    for (const walker of [...walkers]) {
      if (walker.waitUntil) {
        if (now < walker.waitUntil) {
          animateStanding(walker.person, now, false);
          continue;
        }
        walker.waitUntil = 0;
        if (!walker.returning) {
          walker.returning = true;
          // The cup only appears once the walker has reached the dispenser,
          // so it reads as a filled cup rather than a teleport.
          walker.cup.visible = true;
          walker.waypoints = [...walker.waypoints].reverse();
          walker.index = 0;
        } else {
          cancelWalkFor(walker.deskId);
          continue;
        }
      }

      const target = walker.waypoints[walker.index];
      const position = walker.group.position;
      const dx = target.x - position.x;
      const dz = target.z - position.z;
      const distance = Math.hypot(dx, dz);
      if (distance < 0.08) {
        if (walker.index >= walker.waypoints.length - 1) {
          if (!walker.returning) {
            walker.waitUntil = now + walker.waitMs;
            animateStanding(walker.person, now, false);
            continue;
          }
          cancelWalkFor(walker.deskId);
          continue;
        }
        walker.index += 1;
        continue;
      }

      const step = Math.min(distance, walker.speed * dt);
      position.x += (dx / distance) * step;
      position.z += (dz / distance) * step;
      walker.group.rotation.y = Math.atan2(dx, dz);
      animateStanding(walker.person, now, true);
      position.y = FLOOR_Y + Math.abs(Math.sin(now * 0.012)) * 0.025;
    }
  }

  function createPacers() {
    if (REDUCE_MOTION) return;
    const setups = [
      {
        color: '#8d8f88',
        route: [
          [-8.35, -5.6],
          [-8.35, 5.9],
          [0, 5.9],
          [0, -5.6],
        ],
        speed: 0.8,
        start: 0,
      },
      {
        color: '#a8775f',
        route: [
          [0, 4.6],
          [0, -6.3],
          [9.8, -6.3],
          [11.2, -8.6],
          [14.4, -10.2],
          [11.2, -8.6],
          [9.8, -6.3],
        ],
        speed: 0.72,
        start: 2,
      },
      {
        color: '#557a95',
        route: [
          [-8.35, 8.2],
          [-8.35, 15.9],
          [8.35, 15.9],
          [8.35, 8.2],
        ],
        speed: 0.86,
        start: 1,
      },
    ] as const;

    for (const setup of setups) {
      const person = createStandingPerson(setup.color);
      const route = setup.route.map(([x, z]) => new THREE.Vector3(x, FLOOR_Y, z));
      const start = route[setup.start % route.length];
      person.group.position.copy(start);
      scene.add(person.group);
      pacers.push({
        group: person.group,
        person,
        route,
        index: (setup.start + 1) % route.length,
        waitUntil: 0,
        speed: setup.speed,
        phase: Math.random() * Math.PI * 2,
      });
    }
  }

  function updatePacer(pacer: Pacer, now: number, dt: number) {
    if (pacer.waitUntil) {
      if (now < pacer.waitUntil) {
        animateStanding(pacer.person, now + pacer.phase, false);
        return;
      }
      pacer.waitUntil = 0;
    }
    const target = pacer.route[pacer.index];
    const position = pacer.group.position;
    const dx = target.x - position.x;
    const dz = target.z - position.z;
    const distance = Math.hypot(dx, dz);
    if (distance < 0.08) {
      pacer.waitUntil = now + 1300 + Math.random() * 1800;
      pacer.index = (pacer.index + 1) % pacer.route.length;
      return;
    }
    const step = Math.min(distance, pacer.speed * dt);
    position.x += (dx / distance) * step;
    position.z += (dz / distance) * step;
    pacer.group.rotation.y = Math.atan2(dx, dz);
    animateStanding(pacer.person, now + pacer.phase, true);
    position.y = FLOOR_Y + Math.abs(Math.sin(now * 0.01 + pacer.phase)) * 0.02;
  }

  function sync(desks: Desk[]) {
    const present = new Set(desks.map((desk) => desk.id));
    for (const id of [...entries.keys()]) {
      if (!present.has(id)) removeEntry(id);
    }
    for (const id of [...assignments.keys()]) {
      if (!present.has(id)) assignments.delete(id);
    }

    const usedAgent = new Set<number>();
    const usedSub = new Set<number>();
    for (const assignment of assignments.values()) {
      (assignment.area === AREA_SUBAGENT ? usedSub : usedAgent).add(assignment.slot);
    }

    for (const desk of desks) {
      const area = areaFor(desk);
      let assignment = assignments.get(desk.id);
      if (!assignment || assignment.area !== area) {
        const used = area === AREA_SUBAGENT ? usedSub : usedAgent;
        let slot = 0;
        while (used.has(slot)) slot += 1;
        assignment = { area, slot };
        assignments.set(desk.id, assignment);
        used.add(slot);
      }

      const wantsSmall = area === AREA_SUBAGENT;
      let entry = entries.get(desk.id);
      if (entry && entry.small !== wantsSmall) {
        removeEntry(desk.id);
        entry = undefined;
      }

      const [x, z] = slotOffset(area, assignment.slot);
      if (!entry) {
        entry = makeEntry(desk);
        assignments.set(desk.id, assignment);
      }

      entry.state = desk.state;
      entry.group.position.set(x, FLOOR_Y, z);
      entry.torsoMaterial.color.set(shirtFor(desk));
      entry.ledMaterial.color.set(STATE_COLORS[desk.state]);
      entry.ringMaterial.color.set(deskGlowColor(entry));
      // Thought / approval / tool bubbles float above the agent, visible
      // even when an agent sits behind a partition.
      const bubbleKind =
        desk.state === 'awaiting_approval'
          ? ('approval' as const)
          : desk.state === 'executing_tool' || desk.state === 'calling_tool'
            ? ('tool' as const)
            : desk.state === 'thinking'
              ? ('thinking' as const)
              : null;
      if (bubbleKind) {
        if (entry.lastBubbleKind !== bubbleKind) {
          entry.lastBubbleKind = bubbleKind;
          entry.thoughtBubbleUntil = performance.now() + 4200;
          // Detach before disposing: disposal frees the material/texture but
          // leaves the old sprite as a child of the group, so it kept rendering
          // (and its children accumulated) on every bubble-kind change.
          entry.group.remove(entry.thoughtBubble);
          disposeSpritePair(entry.thoughtBubble);
          const replacement = makeThoughtBubble(bubbleKind);
          entry.thoughtBubble = replacement.sprite;
          entry.thoughtBubble.visible = false;
          entry.group.add(entry.thoughtBubble);
        } else if (entry.thoughtBubbleUntil <= performance.now()) {
          entry.thoughtBubbleUntil = performance.now() + 4200;
        }
      } else {
        entry.thoughtBubbleUntil = 0;
        entry.lastBubbleKind = null;
      }
      // Agents stay at their desk at all times; the frame loop decides whether
      // they are asleep. Only a walk hides the seated body.
      entry.sleepSprite.visible = false;
      const walking = walkers.some((row) => row.deskId === desk.id);
      entry.person.group.visible = !walking;
      if (ACTIVE_STATES.has(desk.state)) {
        const current = walkers.find((row) => row.deskId === desk.id);
        // A stroll stops as soon as real work arrives; a report trip already
        // belongs to an active agent and is allowed to finish.
        if (current && current.reason === 'rest') cancelWalkFor(desk.id);
      }
    }
    applySelection();
  }

  function select(id: string) {
    selectedId = id;
    applySelection();
  }

  function hover(id: string) {
    if (hoveredId === id) return;
    hoveredId = id;
    applySelection();
  }

  function focus(id: string) {
    const entry = entries.get(id);
    const target = entry?.group.position.clone().add(new THREE.Vector3(0, 0.9, 0))
      || (id === DIRECTOR_ID ? directorDesk.position.clone().add(new THREE.Vector3(0, 0.9, 0)) : null);
    if (!target) return;
    controls.target.copy(target);
    // Deterministic isometric framing. Adding the target offset to the current
    // camera position left the view unchanged when damping re-applied the old
    // orbit state, so anchor the camera explicitly instead.
    camera.position.set(target.x + 8.5, target.y + 9.5, target.z + 10.5);
    camera.zoom = Math.max(camera.zoom, 3.2);
    camera.updateProjectionMatrix();
    controls.update();
  }

  function reset() {
    controls.target.set(0, 1, -1);
    camera.position.set(31, 29, 37);
    camera.zoom = 1;
    camera.updateProjectionMatrix();
    controls.update();
  }

  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();

  function pick(event: MouseEvent): string {
    const rect = canvas.getBoundingClientRect();
    pointer.x = ((event.clientX - rect.left) / Math.max(1, rect.width)) * 2 - 1;
    pointer.y = -((event.clientY - rect.top) / Math.max(1, rect.height)) * 2 + 1;
    raycaster.setFromCamera(pointer, camera);
    const hits = raycaster.intersectObjects(
      [...entries.values()].map((entry) => entry.group).concat(directorDesk),
      true,
    );
    for (const hit of hits) {
      let node: THREE.Object3D | null = hit.object;
      while (node) {
        const sessionId = node.userData?.sessionId;
        if (typeof sessionId === 'string') return sessionId;
        node = node.parent;
      }
    }
    return '';
  }

  const onPointerDown = (event: PointerEvent) => {
    if (event.button !== 0) return;
    const id = pick(event);
    if (id) {
      select(id);
      onSelect(id);
    }
  };
  const onPointerMove = (event: PointerEvent) => {
    const id = pick(event);
    hover(id);
    canvas.style.cursor = id ? 'pointer' : 'grab';
    onHover?.(id, event.clientX, event.clientY);
  };
  const onPointerLeave = () => {
    hover('');
    canvas.style.cursor = 'grab';
    onHover?.('', 0, 0);
  };
  const onDoubleClick = (event: MouseEvent) => {
    const id = pick(event);
    if (!id) return;
    select(id);
    focus(id);
    onSelect(id);
  };
  canvas.addEventListener('pointerdown', onPointerDown);
  canvas.addEventListener('pointermove', onPointerMove);
  canvas.addEventListener('pointerleave', onPointerLeave);
  canvas.addEventListener('dblclick', onDoubleClick);

  const resizeObserver = new ResizeObserver(() => {
    const parent = canvas.parentElement;
    if (!parent) return;
    const width = parent.clientWidth;
    const height = parent.clientHeight;
    renderer.setSize(width, height, false);
    const aspect = width / Math.max(1, height);
    const half = 21.5;
    camera.left = -half * aspect;
    camera.right = half * aspect;
    camera.top = half;
    camera.bottom = -half;
    camera.updateProjectionMatrix();
  });
  if (canvas.parentElement) resizeObserver.observe(canvas.parentElement);
  if (canvas.parentElement) {
    const parent = canvas.parentElement;
    renderer.setSize(parent.clientWidth, parent.clientHeight, false);
    const aspect = parent.clientWidth / Math.max(1, parent.clientHeight);
    const half = 21.5;
    camera.left = -half * aspect;
    camera.right = half * aspect;
    camera.top = half;
    camera.bottom = -half;
    camera.updateProjectionMatrix();
  }

  // The wandering extras were replaced by the real agent rotation: one agent
  // at a time walks the floor and returns to its own desk.
  // createPacers();

  let hourOfDay = new Date().getHours() + new Date().getMinutes() / 60;
  let lastDaylightAt = 0;
  let lastDaylight = -1;
  let forcedMode: 'auto' | 'day' | 'night' = 'auto';
  let currentDarkness = 0;

  /**
   * The office follows the operator's clock: a warm, bright midday and a cool,
   * dim evening. As daylight falls, ceiling pendants and sconces turn on and
   * the monitors stay lit, so the office never goes black. Recomputing on a
   * 30 s tick (plus every frame while the brightness is settling) keeps this
   * cheap per frame.
   */
  const updateDaylight = (now: number) => {
    if (now - lastDaylightAt > 30_000) {
      lastDaylightAt = now;
      const date = new Date();
      hourOfDay = date.getHours() + date.getMinutes() / 60;
    }
    const daylight = THREE.MathUtils.clamp(
      Math.sin(((hourOfDay - 6) / 12) * Math.PI),
      0.05,
      1,
    );
    // 'day'/'night' pin the lighting regardless of the operator's clock; 'auto'
    // follows it. Evaluating a single effective value keeps every downstream
    // colour/intensity consistent with the chosen mode.
    const effectiveDaylight =
      forcedMode === 'day' ? 1 : forcedMode === 'night' ? 0 : daylight;
    const targetDarkness = 1 - effectiveDaylight;
    if (
      Math.abs(targetDarkness - currentDarkness) < 0.002
      && Math.abs(effectiveDaylight - lastDaylight) < 0.005
    ) {
      return;
    }
    lastDaylight = effectiveDaylight;
    currentDarkness = targetDarkness;
    const dark = currentDarkness;
    const isNight = dark > 0.55;

    skyMaterial.map = isNight ? skyNightTexture : skyTexture;
    skyMaterial.needsUpdate = true;

    // At night the office is lit almost entirely by the warm lamps.
    hemi.intensity = isNight ? 0.05 : 0.22 + effectiveDaylight * 0.58;
    sun.intensity = isNight ? 0.02 : 0.15 + effectiveDaylight * 1.2;
    sun.color.set(effectiveDaylight > 0.45 ? '#fff0d8' : '#7d95b0');
    fill.intensity = isNight ? 0.02 : 0.1 + effectiveDaylight * 0.25;
    skyMaterial.color.setRGB(
      0.35 + effectiveDaylight * 0.65,
      0.40 + effectiveDaylight * 0.60,
      0.50 + effectiveDaylight * 0.50,
    );
    renderer.toneMappingExposure = isNight ? 0.82 : 0.88 + effectiveDaylight * 0.08;
    if (scene.background instanceof THREE.Color) {
      if (isNight) {
        scene.background.setRGB(0.05, 0.07, 0.12);
      } else {
        scene.background.setRGB(
          0.26 + effectiveDaylight * 0.22,
          0.28 + effectiveDaylight * 0.18,
          0.32 + effectiveDaylight * 0.12,
        );
      }
    }

    // Crucial: reduce environment map intensity at night so studio lighting does not overpower the darkness
    if ('environmentIntensity' in scene) {
      (scene as any).environmentIntensity = isNight ? 0.02 : 0.20 + effectiveDaylight * 0.08;
    }

    // Warm lamps fade in as it gets dark; they never fully die.
    const lampLevel = THREE.MathUtils.clamp(dark * 1.9, 0, 1);
    for (const light of lampLights) light.intensity = lampLevel * 45;
    for (const light of ceilingLights) light.intensity = lampLevel * 16;

    // Light pools ride the darkness level: strongest directly under each
    // pendant, gone by day, so the glow follows the fixtures.
    poolMaterial.opacity = lampLevel * 0.9;
    for (const [index, pool] of pools.entries()) {
      const zone = lampZones[index];
      pool.position.set(zone.x, 0.035, zone.z);
    }

    // Bulb meshes (pendants) and sconces get brighter with darkness too.
    for (const bulb of bulbMaterials) {
      bulb.color.set(lampLevel > 0.35 ? '#ffe6a8' : '#fff0c8');
    }
    for (const sconce of sconceMaterials) {
      sconce.color.set(lampLevel > 0.35 ? '#ffdf9e' : '#ffe6b8');
    }

    // Monitor glow stays on regardless; a soft blue pool helps at night.
    for (const entry of entries.values()) {
      entry.ledMaterial.color.set(
        entry.id === selectedId ? '#3f5b70' : STATE_COLORS[entry.state],
      );
    }
  };

  let frameId = 0;
  let last = performance.now();
  const frame = (now: number) => {
    frameId = requestAnimationFrame(frame);
    const dt = Math.min((now - last) / 1000, 0.05);
    last = now;
    controls.update();
    updateDaylight(now);

    // Night mode: pendants gently sway; bulbs brighten with darkness.
    if (!REDUCE_MOTION && currentDarkness > 0.3) {
      for (const [index, bulb] of bulbMeshes.entries()) {
        bulb.position.y =
          lampZones[index].y +
          0.16 +
          Math.sin(now * 0.0006 + index) * 0.008;
      }
    }

    if (!REDUCE_MOTION) {
      for (const entry of entries.values()) {
        const active = ACTIVE_STATES.has(entry.state);
        const sleeping = entry.state === 'sleeping' || entry.state === 'off_shift';
        const walking = walkers.some((row) => row.deskId === entry.id);
        const swing = active ? Math.sin(now * 0.006 + entry.phase) * 0.06 : 0;
        entry.person.leftArm.rotation.x = entry.person.armRestX + swing;
        entry.person.rightArm.rotation.x = entry.person.armRestX - swing;
        entry.person.head.rotation.y = active
          ? Math.sin(now * 0.0012 + entry.phase) * 0.07
          : 0;
        // Asleep: head down, slow breathing, floating z z z. Any task event
        // flips the desk into an active state and wakes the agent instantly.
        entry.person.head.rotation.x = sleeping ? 0.28 : 0;
        entry.sleepSprite.visible = sleeping && !walking;
        if (entry.sleepSprite.visible) {
          const tick = now * 0.0015 + entry.phase;
          entry.sleepSprite.position.y = 2.0 + Math.sin(tick) * 0.08;
          (entry.sleepSprite.material as THREE.SpriteMaterial).opacity =
            0.55 + 0.45 * Math.abs(Math.sin(tick * 0.9));
        }
        if (sleeping && !walking) {
          const breathe = 1 + Math.sin(now * 0.0018 + entry.phase) * 0.012;
          entry.person.group.scale.y = (entry.small ? 0.78 : 1) * breathe;
        } else {
          entry.person.group.scale.y = entry.small ? 0.78 : 1;
        }

        // Thought / approval / tool bubble.
        if (entry.thoughtBubbleUntil > now) {
          const remaining = entry.thoughtBubbleUntil - now;
          const fadeIn = Math.min(1, (4200 - remaining) / 200);
          const fadeOut = Math.min(1, remaining / 400);
          const opacity = Math.min(fadeIn, fadeOut);
          entry.thoughtBubble.visible = opacity > 0;
          entry.thoughtBubble.material.opacity = opacity * 0.98;
          entry.thoughtBubble.position.y =
            2.02 + Math.sin(now * 0.004 + entry.phase) * 0.05;
        } else {
          entry.thoughtBubble.visible = false;
        }
      }
      if (now > nextWalkAt) {
        if (walkers.length < MAX_WALKERS) startWalk(now);
        // Stagger departures so the three walkers do not move in lockstep.
        nextWalkAt = now + 4500 + Math.random() * 4500;
      }
      updateWalkers(now, dt);
      for (const pacer of pacers) updatePacer(pacer, now, dt);
    }

    // Update clock hands from the simulated day.
    const nowDate = new Date();
    const hourAngle = ((nowDate.getHours() % 12) + nowDate.getMinutes() / 60) * (Math.PI / 6);
    const minuteAngle = (nowDate.getMinutes() + nowDate.getSeconds() / 60) * (Math.PI / 30);
    clockHourHand.rotation.z = -hourAngle;
    clockMinuteHand.rotation.z = -minuteAngle;

    const showLabels = camera.zoom < 1.9;
    for (const label of labels) label.visible = showLabels;

    renderer.render(scene, camera);
  };
  frameId = requestAnimationFrame(frame);

  return {
    sync,
    select,
    focus,
    reset,
    setLightMode(mode: 'auto' | 'day' | 'night'): 'auto' | 'day' | 'night' {
      forcedMode = mode;
      // Force a fresh evaluation next frame.
      lastDaylight = -1;
      return forcedMode;
    },
    snapshot(): string {
      renderer.render(scene, camera);
      return canvas.toDataURL('image/png');
    },
    dispose() {
      cancelAnimationFrame(frameId);
      resizeObserver.disconnect();
      canvas.removeEventListener('pointerdown', onPointerDown);
      canvas.removeEventListener('pointermove', onPointerMove);
      canvas.removeEventListener('pointerleave', onPointerLeave);
      canvas.removeEventListener('dblclick', onDoubleClick);
      cancelAllWalks(false);
      controls.dispose();
      disposeObject(scene);
      grid.geometry.dispose();
      (grid.material as THREE.Material).dispose();
      floorTexture.dispose();
      skyTexture.dispose();
      skyNightTexture.dispose();
      poolTexture.dispose();
      poolMaterial.dispose();
      disposeScreenTexture();
      if (scene.environment) {
        (scene.environment as THREE.Texture).dispose();
        scene.environment = null;
      }
      renderer.dispose();
    },
  };
}
