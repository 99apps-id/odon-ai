// Three.js isometric office. Desks exist only for real sessions; the scene
// never fabricates departments, agents, or status.
import * as THREE from 'three';
import { MODE_ROOMS, roomFor, stateColor } from './data.js';

const ROOM_ORDER = ['general', 'code', 'create', 'documents', 'files', 'agents', 'automations'];
const ROOM_POSITIONS = [
  [-12, -8],
  [0, -8],
  [12, -8],
  [-12, 0],
  [12, 0],
  [-12, 8],
  [0, 8],
  [12, 8],
];
const DESK_SLOTS = [
  [-2.7, -1.3],
  [0, -1.3],
  [2.7, -1.3],
  [-2.7, 1.3],
  [0, 1.3],
  [2.7, 1.3],
];

const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

export function createOffice({ canvas, onSelect, onHover }) {
  const renderer = new THREE.WebGLRenderer({
    canvas,
    antialias: true,
    preserveDrawingBuffer: true,
  });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
  renderer.shadowMap.enabled = true;
  renderer.shadowMap.type = THREE.PCFSoftShadowMap;
  renderer.toneMapping = THREE.ACESFilmicToneMapping;
  renderer.toneMappingExposure = 1.05;

  const scene = new THREE.Scene();
  scene.background = new THREE.Color('#f2ede4');

  const camera = new THREE.OrthographicCamera(-20, 20, 16, -16, 0.1, 200);
  camera.position.set(24, 22, 26);
  camera.lookAt(0, 0, 0);

  const hemi = new THREE.HemisphereLight('#fff8ee', '#8a7f6d', 1.25);
  scene.add(hemi);
  const sun = new THREE.DirectionalLight('#fff2dd', 1.9);
  sun.position.set(16, 24, 12);
  sun.castShadow = true;
  sun.shadow.mapSize.set(1024, 1024);
  sun.shadow.camera.left = -30;
  sun.shadow.camera.right = 30;
  sun.shadow.camera.top = 30;
  sun.shadow.camera.bottom = -30;
  sun.shadow.camera.near = 1;
  sun.shadow.camera.far = 80;
  scene.add(sun);

  // Ceiling pendants + pool dots so the standalone night mode reads at a
  // glance, matching the desktop office.
  const lamp = new THREE.Group();
  const lampLights = [];
  function makePoolTexture() {
    const c = document.createElement('canvas');
    c.width = 256; c.height = 256;
    const ctx = c.getContext('2d');
    if (ctx) {
      const g = ctx.createRadialGradient(128, 128, 0, 128, 128, 128);
      g.addColorStop(0, 'rgba(255,225,160,0.52)');
      g.addColorStop(0.45, 'rgba(255,215,140,0.24)');
      g.addColorStop(1, 'rgba(255,210,130,0)');
      ctx.fillStyle = g;
      ctx.fillRect(0, 0, 256, 256);
    }
    const tex = new THREE.CanvasTexture(c);
    tex.colorSpace = THREE.SRGBColorSpace;
    return tex;
  }
  const poolTexture = makePoolTexture();
  const poolMaterial = new THREE.MeshBasicMaterial({
    map: poolTexture,
    transparent: true,
    opacity: 0,
    blending: THREE.AdditiveBlending,
    depthWrite: false,
    toneMapped: false,
  });
  for (const [x, z] of [[-12, -8], [0, -8], [12, -8], [0, 0], [0, 8]]) {
    const rod = new THREE.Mesh(
      new THREE.CylinderGeometry(0.02, 0.02, 0.35, 8),
      new THREE.MeshStandardMaterial({ color: '#5d3d30', roughness: 0.7 }),
    );
    rod.position.set(x, 3.35, z);
    lamp.add(rod);
    const shade = new THREE.Mesh(
      new THREE.ConeGeometry(0.26, 0.22, 16, 1, true),
      new THREE.MeshStandardMaterial({ color: '#5d3d30', roughness: 0.7, side: THREE.DoubleSide }),
    );
    shade.position.set(x, 3.22, z);
    lamp.add(shade);
    const bulb = new THREE.Mesh(
      new THREE.SphereGeometry(0.055, 10, 8),
      new THREE.MeshBasicMaterial({ color: '#fff0c8' }),
    );
    bulb.position.set(x, 3.16, z);
    lamp.add(bulb);
    const pool = new THREE.Mesh(new THREE.PlaneGeometry(6.6, 6.6), poolMaterial);
    pool.rotation.x = -Math.PI / 2;
    pool.position.set(x, 0.035, z);
    pool.renderOrder = 2;
    lamp.add(pool);
    const light = new THREE.PointLight('#ffd9a0', 0, 15, 1.8);
    light.position.set(x, 2.9, z);
    lamp.add(light);
    lampLights.push(light);
  }
  scene.add(lamp);

  // Director desk at the back office (hoverable, focusable).
  const directorGroup = new THREE.Group();
  directorGroup.userData.sessionId = 'director';
  const deskTop = new THREE.Mesh(
    new THREE.BoxGeometry(2.3, 0.1, 1.25),
    new THREE.MeshStandardMaterial({ color: '#e5dccd', roughness: 1 }),
  );
  deskTop.position.set(0, 0.74, 0);
  deskTop.castShadow = true;
  directorGroup.add(deskTop);
  for (const x of [-1.05, 1.05]) for (const z of [-0.52, 0.52]) {
    const leg = new THREE.Mesh(
      new THREE.BoxGeometry(0.08, 0.72, 0.08),
      new THREE.MeshStandardMaterial({ color: '#5d3d30', roughness: 1 }),
    );
    leg.position.set(x, 0.36, z);
    directorGroup.add(leg);
  }
  const directorLabelCanvas = document.createElement('canvas');
  directorLabelCanvas.width = 512;
  directorLabelCanvas.height = 96;
  const dctx = directorLabelCanvas.getContext('2d');
  if (dctx) {
    dctx.fillStyle = 'rgba(255,250,242,0.94)';
    dctx.fillRect(0, 0, 512, 96);
    dctx.fillStyle = '#3a352e';
    dctx.font = '600 40px system-ui, sans-serif';
    dctx.textAlign = 'center';
    dctx.textBaseline = 'middle';
    dctx.fillText('YOU · DIRECTOR', 256, 50);
  }
  const dtex = new THREE.CanvasTexture(directorLabelCanvas);
  dtex.colorSpace = THREE.SRGBColorSpace;
  const dlabel = new THREE.Sprite(
    new THREE.SpriteMaterial({ map: dtex, transparent: true, depthWrite: false }),
  );
  dlabel.scale.set(2.2, 0.42, 1);
  dlabel.position.set(0, 1.68, 0.4);
  directorGroup.add(dlabel);
  directorGroup.position.set(0, 0.05, -13);
  scene.add(directorGroup);

  const ground = new THREE.Mesh(
    new THREE.PlaneGeometry(70, 70),
    new THREE.MeshStandardMaterial({ color: '#efe6d8', roughness: 1 }),
  );
  ground.rotation.x = -Math.PI / 2;
  ground.receiveShadow = true;
  scene.add(ground);

  const grid = new THREE.GridHelper(70, 35, '#d6cfc3', '#e8e3db');
  grid.position.y = 0.01;
  grid.material.opacity = 0.35;
  grid.material.transparent = true;
  scene.add(grid);

  // The Brain: one core at the centre, wired to every room that has desks.
  const brain = new THREE.Group();
  const core = new THREE.Mesh(
    new THREE.SphereGeometry(0.72, 24, 18),
    new THREE.MeshStandardMaterial({
      color: '#3f5b70',
      emissive: '#3f5b70',
      emissiveIntensity: 0.35,
      roughness: 0.35,
      metalness: 0.15,
    }),
  );
  core.position.y = 1.5;
  core.castShadow = true;
  const halo = new THREE.Mesh(
    new THREE.TorusGeometry(1.05, 0.035, 6, 32),
    new THREE.MeshBasicMaterial({ color: '#7495ad' }),
  );
  halo.position.y = 1.5;
  halo.rotation.x = Math.PI / 2;
  brain.add(core, halo);
  scene.add(brain);

  const deskGroups = new Map();
  const rooms = new Map();
  // Distinct room keys not in ROOM_ORDER get their own row so two unknown
  // modes never stack their floors on ROOM_POSITIONS[7].
  let fallbackRoomCount = 0;
  const wires = new THREE.Group();
  scene.add(wires);
  let selectedId = '';

  function ensureRoom(key) {
    if (rooms.has(key)) return rooms.get(key);
    const index = ROOM_ORDER.indexOf(key);
    let position;
    if (index >= 0) {
      position = ROOM_POSITIONS[index];
    } else {
      position = [ROOM_POSITIONS[7][0], ROOM_POSITIONS[7][1] + fallbackRoomCount * 8];
      fallbackRoomCount += 1;
    }
    const floor = new THREE.Mesh(
      new THREE.BoxGeometry(9, 0.24, 6),
      new THREE.MeshStandardMaterial({ color: '#e5dccd', roughness: 1 }),
    );
    floor.position.set(position[0], 0.12, position[1]);
    floor.receiveShadow = true;
    scene.add(floor);
    const accent = new THREE.Mesh(
      new THREE.BoxGeometry(9, 0.05, 0.16),
      new THREE.MeshBasicMaterial({ color: '#d2c4ae' }),
    );
    accent.position.set(position[0], 0.26, position[1] - 2.9);
    scene.add(accent);

    const wire = new THREE.Line(
      new THREE.BufferGeometry().setFromPoints([
        new THREE.Vector3(0, 1.5, 0),
        new THREE.Vector3(position[0], 0.4, position[1]),
      ]),
      new THREE.LineBasicMaterial({ color: '#c9b8a2', transparent: true, opacity: 0.7 }),
    );
    wires.add(wire);

    const room = { key, floor, accent, position, wire };
    rooms.set(key, room);
    return room;
  }

  function createDesk(desk) {
    const group = new THREE.Group();
    group.scale.setScalar(desk.parentId ? 0.78 : 1);

    const wood = new THREE.MeshStandardMaterial({ color: '#e5dccd', roughness: 1 });
    const dark = new THREE.MeshStandardMaterial({ color: '#5d3d30', roughness: 1 });
    const screen = new THREE.MeshStandardMaterial({ color: '#3f5b70', roughness: 0.65 });

    const top = new THREE.Mesh(new THREE.BoxGeometry(1.6, 0.1, 0.9), wood);
    top.position.y = 0.74;
    top.castShadow = true;
    group.add(top);
    for (const x of [-0.66, 0.66]) {
      for (const z of [-0.33, 0.33]) {
        const leg = new THREE.Mesh(new THREE.BoxGeometry(0.08, 0.72, 0.08), dark);
        leg.position.set(x, 0.36, z);
        leg.castShadow = true;
        group.add(leg);
      }
    }

    const monitor = new THREE.Mesh(new THREE.BoxGeometry(0.64, 0.4, 0.06), screen);
    monitor.position.set(0, 1.02, -0.24);
    monitor.rotation.x = -0.12;
    monitor.castShadow = true;
    group.add(monitor);
    const stand = new THREE.Mesh(new THREE.BoxGeometry(0.08, 0.18, 0.08), dark);
    stand.position.set(0, 0.85, -0.24);
    group.add(stand);

    const seat = new THREE.Mesh(new THREE.BoxGeometry(0.44, 0.08, 0.42), dark);
    seat.position.set(0, 0.45, 0.72);
    group.add(seat);
    const back = new THREE.Mesh(new THREE.BoxGeometry(0.44, 0.5, 0.07), dark);
    back.position.set(0, 0.7, 0.93);
    back.castShadow = true;
    group.add(back);

    const avatarMaterial = new THREE.MeshStandardMaterial({
      color: stateColor(desk.state),
      roughness: 0.8,
    });
    const body = new THREE.Mesh(new THREE.CapsuleGeometry(0.18, 0.45, 4, 8), avatarMaterial);
    body.position.set(0, 0.45, 0.54);
    body.castShadow = true;
    group.add(body);
    const head = new THREE.Mesh(
      new THREE.SphereGeometry(0.16, 10, 8),
      new THREE.MeshStandardMaterial({ color: '#d5b9a3', roughness: 0.9 }),
    );
    head.position.set(0, 0.96, 0.54);
    head.castShadow = true;
    group.add(head);

    const ring = new THREE.Mesh(
      new THREE.TorusGeometry(0.33, 0.035, 6, 18),
      new THREE.MeshBasicMaterial({ color: stateColor(desk.state) }),
    );
    ring.rotation.x = Math.PI / 2;
    ring.position.set(0, 0.03, 0.54);
    group.add(ring);

    group.userData = {
      sessionId: desk.id,
      avatarMaterial,
      body,
      ring,
      state: desk.state,
      phase: Math.random() * Math.PI * 2,
      baseScale: desk.parentId ? 0.78 : 1,
    };
    return group;
  }

  function applySelection() {
    for (const [id, group] of deskGroups) {
      const scale = group.userData.baseScale * (id === selectedId ? 1.08 : 1);
      group.scale.setScalar(scale);
      group.userData.ring.material.color.set(
        id === selectedId ? '#3f5b70' : stateColor(group.userData.state),
      );
    }
  }

  function sync(desks) {
    const usage = new Map();
    const roomOf = new Map();
    const seen = new Set();
    const byId = new Map(desks.map((desk) => [desk.id, desk]));

    // Resolve every desk's room first (recursively, with a cycle guard) so a
    // delegated subagent whose parent appears later in the list still lands in
    // its parent's room instead of falling back to its own mode room.
    const resolveRoom = (id, depth) => {
      if (roomOf.has(id)) return roomOf.get(id);
      const desk = byId.get(id);
      const parentId = desk && desk.parentId;
      let roomKey;
      if (parentId && byId.has(parentId) && depth < 8) {
        roomKey = resolveRoom(parentId, depth + 1);
      } else {
        roomKey = roomFor(desk ? desk.mode : 'idle');
      }
      roomOf.set(id, roomKey);
      return roomKey;
    };

    for (const desk of desks) {
      const roomKey = resolveRoom(desk.id, 0);

      const room = ensureRoom(roomKey);
      const slot = usage.get(roomKey) || 0;
      usage.set(roomKey, slot + 1);
      const offset = DESK_SLOTS[slot % DESK_SLOTS.length];
      // Spill extra desks into a new row instead of stacking them exactly on
      // top of the first six slots.
      const row = Math.floor(slot / DESK_SLOTS.length);

      let group = deskGroups.get(desk.id);
      if (!group) {
        group = createDesk(desk);
        deskGroups.set(desk.id, group);
        scene.add(group);
      }
      group.position.set(
        room.position[0] + offset[0],
        0.25,
        room.position[1] + offset[1] + row * 1.7,
      );
      const color = stateColor(desk.state);
      group.userData.state = desk.state;
      group.userData.avatarMaterial.color.set(color);
      group.userData.ring.material.color.set(
        desk.id === selectedId ? '#3f5b70' : color,
      );
      seen.add(desk.id);
    }

    for (const [id, group] of deskGroups) {
      if (seen.has(id)) continue;
      scene.remove(group);
      group.traverse((node) => {
        if (node.isMesh) {
          node.geometry?.dispose?.();
          if (Array.isArray(node.material)) node.material.forEach((m) => m.dispose?.());
          else node.material?.dispose?.();
        }
      });
      deskGroups.delete(id);
    }
    applySelection();
  }

  function select(id) {
    selectedId = id;
    applySelection();
  }

  const raycaster = new THREE.Raycaster();
  const pointer = new THREE.Vector2();

  function pick(event) {
    const rect = canvas.getBoundingClientRect();
    pointer.x = ((event.clientX - rect.left) / Math.max(1, rect.width)) * 2 - 1;
    pointer.y = -((event.clientY - rect.top) / Math.max(1, rect.height)) * 2 + 1;
    raycaster.setFromCamera(pointer, camera);
    const hits = raycaster.intersectObjects([...deskGroups.values(), directorGroup], true);
    for (const hit of hits) {
      let node = hit.object;
      while (node) {
        if (node.userData?.sessionId) return node.userData.sessionId;
        node = node.parent;
      }
    }
    return '';
  }

  const onPointerDown = (event) => {
    const id = pick(event);
    if (id) {
      select(id);
      onSelect?.(id);
    }
  };
  const onPointerMove = (event) => {
    const id = pick(event);
    canvas.style.cursor = id ? 'pointer' : 'grab';
    onHover?.(id, event.clientX, event.clientY);
  };
  canvas.addEventListener('pointerdown', onPointerDown);
  canvas.addEventListener('pointermove', onPointerMove);

  function resize() {
    const parent = canvas.parentElement;
    const width = parent.clientWidth;
    const height = parent.clientHeight;
    renderer.setSize(width, height, false);
    const aspect = width / Math.max(1, height);
    const half = 18;
    camera.left = -half * aspect;
    camera.right = half * aspect;
    camera.top = half;
    camera.bottom = -half;
    camera.updateProjectionMatrix();
  }
  const resizeObserver = new ResizeObserver(resize);
  resizeObserver.observe(canvas.parentElement);
  resize();

  function focus(id) {
    let target = null;
    if (id === 'director') {
      target = directorGroup.position.clone().add(new THREE.Vector3(0, 0.9, 0));
    } else {
      const group = deskGroups.get(id);
      if (!group) return;
      target = group.position.clone().add(new THREE.Vector3(0, 0.9, 0));
    }
    camera.position.set(target.x + 8.5, target.y + 9.5, target.z + 10.5);
    camera.zoom = Math.max(camera.zoom, 3.2);
    camera.updateProjectionMatrix();
  }

  let nightForced = false;
  let lastDaylight = -1;
  let currentDarkness = 0;
  function updateDaylight(now) {
    const date = new Date();
    const hour = date.getHours() + date.getMinutes() / 60;
    const daylight = Math.min(1, Math.max(0.05, Math.sin(((hour - 6) / 12) * Math.PI)));
    const targetDarkness = nightForced ? 1 : 1 - daylight;
    if (Math.abs(targetDarkness - currentDarkness) < 0.002 && Math.abs(daylight - lastDaylight) < 0.005) return;
    lastDaylight = daylight;
    currentDarkness = targetDarkness;
    const dark = currentDarkness;
    const isNight = dark > 0.55;
    hemi.intensity = isNight ? 0.18 : 0.3 + daylight * 0.75;
    sun.intensity = isNight ? 0.08 : 0.25 + daylight * 1.75;
    sun.color.set(daylight > 0.45 ? '#fff0d8' : '#a9bed6');
    renderer.toneMappingExposure = isNight ? 0.9 : 0.84 + daylight * 0.24;
    scene.background.setRGB(0.28 + daylight * 0.27, 0.32 + daylight * 0.21, 0.38 + daylight * 0.1);
    const lampLevel = Math.min(1, Math.max(0, dark * 1.9));
    for (const light of lampLights) light.intensity = lampLevel * 38;
    poolMaterial.opacity = lampLevel * 0.85;
  }

  let frameId = 0;
  let last = performance.now();
  function frame(now) {
    frameId = requestAnimationFrame(frame);
    const dt = Math.min((now - last) / 1000, 0.05);
    last = now;
    updateDaylight(now);

    if (!reduceMotion) {
      brain.rotation.y += dt * 0.2;
      halo.rotation.z += dt * 0.4;
      for (const group of deskGroups.values()) {
        const active =
          group.userData.state === 'thinking' || group.userData.state === 'executing_tool';
        group.userData.body.position.y = active
          ? 0.45 + Math.sin(now * 0.004 + group.userData.phase) * 0.025
          : 0.45;
      }
    }
    renderer.render(scene, camera);
  }
  frameId = requestAnimationFrame(frame);

  return {
    sync,
    select,
    focus,
    setNight(active) {
      nightForced = active;
      lastDaylight = -1;
      return nightForced;
    },
    snapshot() {
      renderer.render(scene, camera);
      return canvas.toDataURL('image/png');
    },
    roomLabel: (key) => MODE_ROOMS[key] || MODE_ROOMS.general,
    dispose() {
      cancelAnimationFrame(frameId);
      resizeObserver.disconnect();
      canvas.removeEventListener('pointerdown', onPointerDown);
      canvas.removeEventListener('pointermove', onPointerMove);
      poolTexture.dispose();
      poolMaterial.dispose();
      renderer.dispose();
    },
  };
}
