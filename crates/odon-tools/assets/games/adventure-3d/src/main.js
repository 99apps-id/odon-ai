import * as THREE from 'three';
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js';

const canvas = document.getElementById('scene');
const coinsLabel = document.getElementById('coins');
const hintEl = document.getElementById('hint');
const winEl = document.getElementById('win');
const winDetail = document.getElementById('win-detail');
const againButton = document.getElementById('again');

const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
const SPEED = 5.5;
const GRAVITY = 22;
const JUMP = 8.5;
const PLAYER_RADIUS = 0.35;
const GOAL = { x: 0, z: -16, radius: 1.8 };

const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.toneMappingExposure = 1.05;

const scene = new THREE.Scene();
scene.background = new THREE.Color('#f2ede4');
scene.fog = new THREE.Fog('#f2ede4', 30, 62);

const camera = new THREE.PerspectiveCamera(48, 1, 0.1, 120);

scene.add(new THREE.HemisphereLight('#fff8ee', '#8a7f6d', 1.15));
const sun = new THREE.DirectionalLight('#fff2dd', 2.1);
sun.position.set(12, 18, 9);
sun.castShadow = true;
sun.shadow.mapSize.set(2048, 2048);
sun.shadow.camera.left = -26;
sun.shadow.camera.right = 26;
sun.shadow.camera.top = 26;
sun.shadow.camera.bottom = -26;
sun.shadow.camera.near = 1;
sun.shadow.camera.far = 70;
scene.add(sun);

const ground = new THREE.Mesh(
  new THREE.PlaneGeometry(60, 60),
  new THREE.MeshStandardMaterial({ color: '#e5dccd', roughness: 1 }),
);
ground.rotation.x = -Math.PI / 2;
ground.receiveShadow = true;
scene.add(ground);

const grid = new THREE.GridHelper(60, 30, '#d6cfc3', '#e8e3db');
grid.position.y = 0.01;
grid.material.opacity = 0.35;
grid.material.transparent = true;
scene.add(grid);

const PLATFORMS = [
  { x: 0, z: 0, w: 7, d: 7, top: 0.4, h: 0.8, color: '#d2c4ae' },
  { x: -6.5, z: -4, w: 4.5, d: 4.5, top: 1.2, h: 1.2, color: '#e5dccd' },
  { x: 6.5, z: -6, w: 5, d: 5, top: 2.2, h: 2.2, color: '#d2c4ae' },
  { x: 0, z: -11, w: 6, d: 6, top: 1.0, h: 1.0, color: '#e5dccd' },
  { x: 10, z: 3, w: 4, d: 4, top: 0.8, h: 0.8, color: '#d2c4ae' },
];

for (const platform of PLATFORMS) {
  const mesh = new THREE.Mesh(
    new THREE.BoxGeometry(platform.w, platform.h, platform.d),
    new THREE.MeshStandardMaterial({ color: platform.color, roughness: 0.95 }),
  );
  mesh.position.set(platform.x, platform.top - platform.h / 2, platform.z);
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  scene.add(mesh);
}

function addTree(x, z, scale = 1) {
  const group = new THREE.Group();
  const trunk = new THREE.Mesh(
    new THREE.CylinderGeometry(0.16 * scale, 0.22 * scale, 1.2 * scale, 7),
    new THREE.MeshStandardMaterial({ color: '#5d3d30', roughness: 1 }),
  );
  trunk.position.y = 0.6 * scale;
  const canopy = new THREE.Mesh(
    new THREE.ConeGeometry(0.9 * scale, 2.1 * scale, 7),
    new THREE.MeshStandardMaterial({ color: '#3f5d3a', roughness: 1 }),
  );
  canopy.position.y = 2.0 * scale;
  canopy.castShadow = true;
  trunk.castShadow = true;
  group.add(trunk, canopy);
  group.position.set(x, 0, z);
  scene.add(group);
}

addTree(-13, -2, 1.1);
addTree(13, -1, 0.9);
addTree(-11, 8, 0.8);
addTree(9, 10, 1.0);
addTree(15, -10, 1.2);

for (const [x, z, size] of [
  [4, 5, 0.45],
  [-4, 7, 0.3],
  [12, -4, 0.5],
]) {
  const rock = new THREE.Mesh(
    new THREE.DodecahedronGeometry(size, 0),
    new THREE.MeshStandardMaterial({ color: '#b7a68c', roughness: 1, flatShading: true }),
  );
  rock.position.set(x, size * 0.6, z);
  rock.rotation.set(0.4, x, 0.2);
  rock.castShadow = true;
  rock.receiveShadow = true;
  scene.add(rock);
}

// Goal pad.
const goalPad = new THREE.Mesh(
  new THREE.CylinderGeometry(1.4, 1.4, 0.12, 24),
  new THREE.MeshStandardMaterial({
    color: '#047857',
    emissive: '#047857',
    emissiveIntensity: 0.25,
    roughness: 0.6,
  }),
);
goalPad.position.set(GOAL.x, 0.06, GOAL.z);
goalPad.receiveShadow = true;
scene.add(goalPad);

const goalRing = new THREE.Mesh(
  new THREE.TorusGeometry(1.0, 0.07, 8, 28),
  new THREE.MeshStandardMaterial({
    color: '#eab308',
    emissive: '#eab308',
    emissiveIntensity: 0.45,
    roughness: 0.4,
  }),
);
goalRing.rotation.x = Math.PI / 2;
goalRing.position.set(GOAL.x, 1.0, GOAL.z);
scene.add(goalRing);

// Player.
const playerGroup = new THREE.Group();
playerGroup.position.set(0, 0, 6);
scene.add(playerGroup);

const placeholder = new THREE.Mesh(
  new THREE.CapsuleGeometry(PLAYER_RADIUS, 0.8, 6, 12),
  new THREE.MeshStandardMaterial({ color: '#a8775f', roughness: 0.8 }),
);
placeholder.position.y = 0.75;
placeholder.castShadow = true;
playerGroup.add(placeholder);

new GLTFLoader().load(
  './assets/models/sample.gltf',
  (gltf) => {
    const model = gltf.scene;
    model.traverse((child) => {
      if (child.isMesh) {
        child.castShadow = true;
        child.receiveShadow = true;
      }
    });
    playerGroup.add(model);
    placeholder.visible = false;
  },
  undefined,
  () => {
    // Keep the capsule fallback when the model is unavailable.
  },
);

// Rings.
const COIN_LAYOUT = [
  { x: -6.5, y: 2.4, z: -4 },
  { x: 6.5, y: 3.4, z: -6 },
  { x: 0, y: 2.2, z: -11 },
  { x: 10, y: 2.0, z: 3 },
  { x: -2, y: 1.4, z: 3 },
];
const ringGeometry = new THREE.TorusGeometry(0.24, 0.08, 8, 18);
const ringMaterial = new THREE.MeshStandardMaterial({
  color: '#b45309',
  emissive: '#b45309',
  emissiveIntensity: 0.25,
  roughness: 0.4,
  metalness: 0.3,
});
const rings = COIN_LAYOUT.map((layout, index) => {
  const mesh = new THREE.Mesh(ringGeometry, ringMaterial);
  mesh.position.set(layout.x, layout.y, layout.z);
  mesh.rotation.x = Math.PI / 2;
  mesh.castShadow = true;
  scene.add(mesh);
  return { ...layout, mesh, collected: false, phase: index * 0.7 };
});

const player = { x: 0, y: 0, z: 6, vx: 0, vz: 0, vy: 0, onGround: false };
const keys = new Set();
let jumpQueued = false;
let collected = 0;
let won = false;
let startedAt = performance.now();
let hintTimer;

function showHint(text) {
  hintEl.textContent = text;
  hintEl.classList.remove('hidden');
  window.clearTimeout(hintTimer);
  hintTimer = window.setTimeout(() => hintEl.classList.add('hidden'), 1800);
}

function groundHeightAt(x, z, feetY) {
  let best = 0;
  for (const platform of PLATFORMS) {
    if (Math.abs(x - platform.x) > platform.w / 2 + PLAYER_RADIUS) continue;
    if (Math.abs(z - platform.z) > platform.d / 2 + PLAYER_RADIUS) continue;
    if (platform.top <= feetY + 0.45 && platform.top > best) best = platform.top;
  }
  return best;
}

function blocked(x, z, feetY) {
  for (const platform of PLATFORMS) {
    if (Math.abs(x - platform.x) > platform.w / 2 + 0.3) continue;
    if (Math.abs(z - platform.z) > platform.d / 2 + 0.3) continue;
    if (platform.top > feetY + 0.5 && platform.top - platform.h < feetY + 1.7) return true;
  }
  return false;
}

function resetPlayer() {
  player.x = 0;
  player.y = 0;
  player.z = 6;
  player.vx = 0;
  player.vz = 0;
  player.vy = 0;
  player.onGround = false;
}

function restart() {
  resetPlayer();
  for (const ring of rings) {
    ring.collected = false;
    ring.mesh.visible = true;
  }
  collected = 0;
  won = false;
  startedAt = performance.now();
  winEl.classList.add('hidden');
}

window.addEventListener('keydown', (event) => {
  if (['KeyW', 'KeyA', 'KeyS', 'KeyD', 'Space', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight'].includes(event.code)) {
    event.preventDefault();
  }
  if (event.repeat) return;
  keys.add(event.code);
  if (event.code === 'Space') jumpQueued = true;
  if (event.code === 'KeyR') restart();
});

window.addEventListener('keyup', (event) => keys.delete(event.code));
window.addEventListener('blur', () => keys.clear());
againButton.addEventListener('click', restart);

function update(dt) {
  const direction = new THREE.Vector2(0, 0);
  if (keys.has('KeyW') || keys.has('ArrowUp')) direction.y -= 1;
  if (keys.has('KeyS') || keys.has('ArrowDown')) direction.y += 1;
  if (keys.has('KeyA') || keys.has('ArrowLeft')) direction.x -= 1;
  if (keys.has('KeyD') || keys.has('ArrowRight')) direction.x += 1;
  if (direction.lengthSq() > 0) direction.normalize();

  const smooth = 1 - Math.exp(-14 * dt);
  player.vx += (direction.x * SPEED - player.vx) * smooth;
  player.vz += (direction.y * SPEED - player.vz) * smooth;

  const nextX = player.x + player.vx * dt;
  if (!blocked(nextX, player.z, player.y)) player.x = nextX;
  else player.vx = 0;
  const nextZ = player.z + player.vz * dt;
  if (!blocked(player.x, nextZ, player.y)) player.z = nextZ;
  else player.vz = 0;

  if (jumpQueued && player.onGround) {
    player.vy = JUMP;
    player.onGround = false;
  }
  jumpQueued = false;

  const previousY = player.y;
  player.vy -= GRAVITY * dt;
  player.y += player.vy * dt;
  const floor = groundHeightAt(player.x, player.z, previousY);
  if (player.y <= floor && player.vy <= 0) {
    player.y = floor;
    player.vy = 0;
    player.onGround = true;
  } else {
    player.onGround = false;
  }

  playerGroup.position.set(player.x, player.y, player.z);
  if (Math.abs(player.vx) + Math.abs(player.vz) > 0.15) {
    playerGroup.rotation.y = Math.atan2(player.vx, player.vz);
  }

  for (const ring of rings) {
    if (ring.collected) continue;
    const dx = player.x - ring.x;
    const dz = player.z - ring.z;
    const dy = player.y + 0.9 - ring.y;
    if (dx * dx + dz * dz < 1.3 * 1.3 && Math.abs(dy) < 1.5) {
      ring.collected = true;
      ring.mesh.visible = false;
      collected += 1;
      showHint(`Ring ${collected}/${rings.length}`);
    }
  }

  const atGoal = Math.hypot(player.x - GOAL.x, player.z - GOAL.z) < GOAL.radius;
  if (!won && atGoal && collected === rings.length) {
    won = true;
    const seconds = (performance.now() - startedAt) / 1000;
    winDetail.textContent = `All ${rings.length} rings · ${seconds.toFixed(1)}s`;
    winEl.classList.remove('hidden');
  } else if (!won && atGoal && collected < rings.length) {
    showHint(`Collect every ring first (${collected}/${rings.length}).`);
  }

  coinsLabel.textContent = `Rings ${collected}/${rings.length}`;
}

function animate(dt) {
  if (reduceMotion) return;
  for (const ring of rings) {
    if (!ring.collected) ring.mesh.rotation.z += dt * 1.4;
  }
  goalRing.rotation.z += dt * 0.5;
}

function resize() {
  const parent = canvas.parentElement;
  const width = parent.clientWidth;
  const height = parent.clientHeight;
  renderer.setSize(width, height, false);
  camera.aspect = width / Math.max(1, height);
  camera.updateProjectionMatrix();
}

new ResizeObserver(resize).observe(canvas.parentElement);
resize();

const clock = new THREE.Clock();
const desiredCamera = new THREE.Vector3();
const lookAt = new THREE.Vector3();

function frame() {
  requestAnimationFrame(frame);
  const dt = Math.min(clock.getDelta(), 0.05);
  if (!won) update(dt);
  animate(dt);

  desiredCamera.set(player.x + 8, player.y + 6.5, player.z + 10);
  if (reduceMotion) camera.position.copy(desiredCamera);
  else camera.position.lerp(desiredCamera, 1 - Math.exp(-4 * dt));
  lookAt.set(player.x, player.y + 1, player.z);
  camera.lookAt(lookAt);

  renderer.render(scene, camera);
}

requestAnimationFrame(frame);
