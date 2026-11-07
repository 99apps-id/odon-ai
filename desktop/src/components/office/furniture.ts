import * as THREE from 'three';
import { RoundedBoxGeometry } from 'three/examples/jsm/geometries/RoundedBoxGeometry.js';

/**
 * Small, dependency-free furniture builders for the office. Everything is
 * primitives with the ODON paper-and-clay palette; geometry is intentionally
 * low-poly so a dozen desks stay cheap to render.
 */

export const PALETTE = {
  floor: '#bab3a7',
  floorAgent: '#b5ada1',
  floorSub: '#b8b2a8',
  floorDirector: '#b0a89c',
  floorMeeting: '#b3aca0',
  floorLounge: '#b8b0a4',
  wall: '#cfc8bd',
  wallTrim: '#9e9587',
  glass: '#cfe3ee',
  wood: '#c9bead',
  woodDark: '#5d3d30',
  ink: '#3a352e',
  skin: '#d5b9a3',
  hair: '#4a3127',
  green: '#3f5d3a',
  greenLight: '#5f7d58',
  terracotta: '#a8775f',
  blue: '#3f5b70',
  white: '#dfd8cc',
} as const;

export function material(color: string, roughness = 0.9, metalness = 0): THREE.MeshStandardMaterial {
  return new THREE.MeshStandardMaterial({ color, roughness, metalness });
}

export function box(
  width: number,
  height: number,
  depth: number,
  color: string,
  x = 0,
  y = 0,
  z = 0,
): THREE.Mesh {
  const mesh = new THREE.Mesh(new THREE.BoxGeometry(width, height, depth), material(color));
  mesh.position.set(x, y, z);
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  return mesh;
}

export function cylinder(
  radiusTop: number,
  radiusBottom: number,
  height: number,
  color: string,
  segments = 14,
): THREE.Mesh {
  const mesh = new THREE.Mesh(
    new THREE.CylinderGeometry(radiusTop, radiusBottom, height, segments),
    material(color),
  );
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  return mesh;
}

export function sphere(radius: number, color: string, segments = 12): THREE.Mesh {
  const mesh = new THREE.Mesh(new THREE.SphereGeometry(radius, segments, segments), material(color));
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  return mesh;
}

/** A camera-facing label, drawn once onto a canvas texture. */
export function makeLabel(text: string, size = 0.55): THREE.Sprite {
  const font = '600 44px system-ui, "Segoe UI", sans-serif';
  const measure = document.createElement('canvas').getContext('2d');
  if (!measure) return new THREE.Sprite();
  measure.font = font;
  const width = Math.max(180, Math.ceil(measure.measureText(text).width) + 56);
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = 96;
  const ctx = canvas.getContext('2d');
  if (!ctx) return new THREE.Sprite();
  ctx.clearRect(0, 0, width, 96);
  ctx.fillStyle = 'rgba(255, 250, 242, 0.94)';
  ctx.strokeStyle = 'rgba(214, 207, 195, 0.95)';
  ctx.lineWidth = 2;
  ctx.beginPath();
  if (ctx.roundRect) ctx.roundRect(4, 12, width - 8, 72, 26);
  else ctx.rect(4, 12, width - 8, 72);
  ctx.fill();
  ctx.stroke();
  ctx.fillStyle = PALETTE.ink;
  ctx.font = font;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(text.toUpperCase(), width / 2, 50);
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.anisotropy = 4;
  const sprite = new THREE.Sprite(
    new THREE.SpriteMaterial({ map: texture, transparent: true, depthWrite: false }),
  );
  sprite.scale.set(size * (width / 96), size, 1);
  return sprite;
}

export interface Person {
  group: THREE.Group;
  torsoMaterial: THREE.MeshStandardMaterial;
  leftArm: THREE.Object3D;
  rightArm: THREE.Object3D;
  leftLeg: THREE.Object3D;
  rightLeg: THREE.Object3D;
  head: THREE.Mesh;
  /** Resting shoulder rotation, so callers can animate around it. */
  armRestX: number;
}

const SKIN_TONES = ['#e8c4a2', '#d5a97e', '#b9845f', '#8d5a3b'];
const HAIR_TONES = ['#2b2721', '#4a3127', '#6b4a2f', '#1f1c19'];

function hashColor(value: string): number {
  let hash = 0;
  for (let index = 0; index < value.length; index += 1) {
    hash = (hash * 31 + value.charCodeAt(index)) >>> 0;
  }
  return hash;
}

function personLook(tint: string, director: boolean) {
  const hash = hashColor(tint);
  return {
    skin: director ? '#e8c4a2' : SKIN_TONES[hash % SKIN_TONES.length],
    hair: director ? '#8d8a80' : HAIR_TONES[(hash >>> 3) % HAIR_TONES.length],
    style: director ? 0 : (hash >>> 6) % 2,
  };
}

function capsule(
  radius: number,
  length: number,
  color: string,
  segments = 10,
): THREE.Mesh {
  const mesh = new THREE.Mesh(
    new THREE.CapsuleGeometry(radius, length, 5, segments),
    material(color, 0.8),
  );
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  return mesh;
}

function addFace(group: THREE.Group, skin: string, hair: string, style: number, yOffset = 0) {
  const head = sphere(0.165, skin, 18);
  head.scale.set(0.96, 1.06, 0.98);
  head.position.set(0, 1.34 + yOffset, 0.03);
  group.add(head);

  for (const side of [-1, 1]) {
    const ear = sphere(0.035, skin, 8);
    ear.position.set(side * 0.165, 1.33 + yOffset, 0.02);
    group.add(ear);
    const eye = sphere(0.017, '#2b2721', 8);
    eye.position.set(side * 0.055, 1.36 + yOffset, 0.172);
    group.add(eye);
    const brow = box(0.05, 0.012, 0.012, hair, side * 0.055, 1.405 + yOffset, 0.17);
    group.add(brow);
  }
  const nose = sphere(0.026, skin, 8);
  nose.position.set(0, 1.325 + yOffset, 0.188);
  group.add(nose);
  group.add(box(0.055, 0.012, 0.01, '#8d5f49', 0, 1.28 + yOffset, 0.175));

  const cap = sphere(0.172, hair, 14);
  cap.scale.set(1, 0.62, 1);
  cap.position.set(0, 1.425 + yOffset, 0.0);
  group.add(cap);
  if (style === 1) {
    const backHair = box(0.28, 0.18, 0.08, hair, 0, 1.33 + yOffset, -0.12);
    group.add(backHair);
  } else {
    const fringe = box(0.2, 0.05, 0.05, hair, 0, 1.44 + yOffset, 0.13);
    fringe.rotation.x = -0.18;
    group.add(fringe);
  }
  return head;
}

/**
 * A seated person facing local +Z. The caller rotates the group so the person
 * faces the desk; agents sit with their back to the camera, the director faces
 * the camera.
 */
export function createSeatedPerson(color: string, director = false): Person {
  const group = new THREE.Group();
  const look = personLook(color, director);
  const torsoMaterial = material(color, 0.82);
  const pantsColor = director ? '#2b3038' : '#3a3f45';
  const skin = look.skin;

  // Office chair: seat, back, armrests, post and a five-star base.
  group.add(box(0.5, 0.08, 0.46, '#3a352e', 0, 0.45, -0.14));
  const chairBack = box(0.48, 0.44, 0.09, '#3a352e', 0, 0.72, -0.39);
  chairBack.rotation.x = -0.08;
  group.add(chairBack);
  for (const side of [-1, 1]) {
    group.add(box(0.06, 0.06, 0.42, '#2b2721', side * 0.29, 0.66, -0.14));
  }
  const post = cylinder(0.045, 0.045, 0.34, '#2b2721', 8);
  post.position.set(0, 0.24, -0.14);
  group.add(post);
  const base = cylinder(0.26, 0.26, 0.04, '#2b2721', 14);
  base.position.set(0, 0.06, -0.14);
  group.add(base);
  for (let index = 0; index < 5; index += 1) {
    const angle = (index / 5) * Math.PI * 2;
    const wheel = sphere(0.035, '#1f1c19', 8);
    wheel.position.set(Math.cos(angle) * 0.24, 0.035, Math.sin(angle) * 0.24 - 0.14);
    group.add(wheel);
  }

  // Hips and a rounded torso.
  group.add(box(0.36, 0.2, 0.32, pantsColor, 0, 0.51, 0));
  const torso = new THREE.Mesh(new THREE.CapsuleGeometry(0.19, 0.26, 6, 14), torsoMaterial);
  torso.scale.z = 0.82;
  torso.position.set(0, 0.88, 0.02);
  torso.castShadow = true;
  group.add(torso);

  // Collar, lanyard and badge (a tie for the director).
  group.add(box(0.16, 0.07, 0.025, '#fffaf2', 0, 1.11, 0.175));
  if (director) {
    group.add(box(0.06, 0.3, 0.025, '#8d5f49', 0, 0.98, 0.195));
  } else {
    group.add(box(0.025, 0.28, 0.015, '#2b2721', 0, 0.99, 0.195));
    group.add(box(0.11, 0.075, 0.02, '#c9d7e1', 0, 0.87, 0.2));
    group.add(box(0.06, 0.018, 0.005, PALETTE.blue, 0, 0.895, 0.212));
  }

  const neck = cylinder(0.055, 0.06, 0.1, skin, 10);
  neck.position.set(0, 1.17, 0.03);
  group.add(neck);
  const head = addFace(group, skin, look.hair, look.style);

  // Shoulder-pivoted arms resting on the desk.
  const armRestX = -1.02;
  const buildArm = (side: number) => {
    const pivot = new THREE.Group();
    pivot.position.set(side * 0.235, 1.09, 0.03);
    const upper = capsule(0.058, 0.16, color, 8);
    upper.position.y = -0.14;
    pivot.add(upper);
    const elbow = new THREE.Group();
    elbow.position.y = -0.29;
    pivot.add(elbow);
    const fore = capsule(0.047, 0.16, skin, 8);
    fore.position.y = -0.12;
    elbow.add(fore);
    const hand = sphere(0.055, skin, 8);
    hand.position.set(0, -0.25, 0.01);
    elbow.add(hand);
    pivot.rotation.x = armRestX;
    pivot.rotation.z = side * 0.14;
    elbow.rotation.x = -0.52;
    group.add(pivot);
    return pivot;
  };
  const leftArm = buildArm(-1);
  const rightArm = buildArm(1);

  // Seated legs: thighs forward, knees bent, shoes on the floor.
  let leftLeg: THREE.Object3D = group;
  let rightLeg: THREE.Object3D = group;
  for (const side of [-1, 1]) {
    const thigh = capsule(0.075, 0.24, pantsColor, 8);
    thigh.rotation.x = Math.PI / 2;
    thigh.position.set(side * 0.105, 0.5, 0.18);
    group.add(thigh);
    const knee = sphere(0.08, pantsColor, 8);
    knee.position.set(side * 0.105, 0.5, 0.34);
    group.add(knee);
    const shin = capsule(0.06, 0.28, pantsColor, 8);
    shin.position.set(side * 0.105, 0.27, 0.36);
    group.add(shin);
    group.add(box(0.13, 0.08, 0.26, '#2b2721', side * 0.105, 0.05, 0.42));
    if (side < 0) leftLeg = shin;
    else rightLeg = shin;
  }

  return {
    group,
    torsoMaterial,
    leftArm,
    rightArm,
    leftLeg,
    rightLeg,
    head,
    armRestX,
  };
}

/** A standing person for the ambient coffee walk. Origin at the feet. */
export function createStandingPerson(color: string): Person {
  const group = new THREE.Group();
  const look = personLook(color, false);
  const torsoMaterial = material(color, 0.82);
  const pantsColor = '#3a3f45';

  group.add(box(0.34, 0.2, 0.26, pantsColor, 0, 0.9, 0));
  const torso = new THREE.Mesh(new THREE.CapsuleGeometry(0.19, 0.3, 6, 14), torsoMaterial);
  torso.scale.z = 0.8;
  torso.position.set(0, 1.14, 0);
  torso.castShadow = true;
  group.add(torso);
  group.add(box(0.16, 0.07, 0.025, '#fffaf2', 0, 1.4, 0.16));
  group.add(box(0.025, 0.3, 0.015, '#2b2721', 0, 1.27, 0.18));
  group.add(box(0.11, 0.075, 0.02, '#c9d7e1', 0, 1.14, 0.185));

  const neck = cylinder(0.055, 0.06, 0.1, look.skin, 10);
  neck.position.set(0, 1.47, 0.02);
  group.add(neck);
  const head = addFace(group, look.skin, look.hair, look.style, 0.26);

  const buildArm = (side: number) => {
    const pivot = new THREE.Group();
    pivot.position.set(side * 0.235, 1.38, 0.01);
    const upper = capsule(0.058, 0.18, color, 8);
    upper.position.y = -0.15;
    pivot.add(upper);
    const elbow = new THREE.Group();
    elbow.position.y = -0.29;
    pivot.add(elbow);
    const fore = capsule(0.047, 0.16, look.skin, 8);
    fore.position.y = -0.12;
    elbow.add(fore);
    const hand = sphere(0.055, look.skin, 8);
    hand.position.y = -0.23;
    elbow.add(hand);
    pivot.rotation.x = 0.12;
    pivot.rotation.z = side * 0.08;
    elbow.rotation.x = -0.22;
    group.add(pivot);
    return pivot;
  };
  const leftArm = buildArm(-1);
  const rightArm = buildArm(1);

  const buildLeg = (side: number) => {
    const hip = new THREE.Group();
    hip.position.set(side * 0.1, 0.88, 0);
    const thigh = capsule(0.075, 0.3, pantsColor, 8);
    thigh.position.y = -0.2;
    hip.add(thigh);
    const knee = new THREE.Group();
    knee.position.y = -0.38;
    hip.add(knee);
    const shin = capsule(0.06, 0.3, pantsColor, 8);
    shin.position.y = -0.2;
    knee.add(shin);
    knee.add(box(0.13, 0.08, 0.26, '#2b2721', 0, -0.43, 0.05));
    group.add(hip);
    return hip;
  };
  const leftLeg = buildLeg(-1);
  const rightLeg = buildLeg(1);

  return {
    group,
    torsoMaterial,
    leftArm,
    rightArm,
    leftLeg,
    rightLeg,
    head,
    armRestX: 0.12,
  };
}

export interface DeskVisual {
  group: THREE.Group;
  person: Person;
  torsoMaterial: THREE.MeshStandardMaterial;
  ringMaterial: THREE.MeshBasicMaterial;
  ledMaterial: THREE.MeshBasicMaterial;
  small: boolean;
}

let screenTexture: THREE.CanvasTexture | null = null;

/** A glowing editor screen shared by every monitor. */
function getScreenTexture(): THREE.CanvasTexture {
  if (screenTexture) return screenTexture;
  const canvas = document.createElement('canvas');
  canvas.width = 512;
  canvas.height = 320;
  const ctx = canvas.getContext('2d');
  if (ctx) {
    const backdrop = ctx.createLinearGradient(0, 0, 0, 320);
    backdrop.addColorStop(0, '#22323d');
    backdrop.addColorStop(1, '#162129');
    ctx.fillStyle = backdrop;
    ctx.fillRect(0, 0, 512, 320);

    ctx.fillStyle = '#2b3c49';
    ctx.fillRect(0, 0, 512, 34);
    for (let index = 0; index < 3; index += 1) {
      ctx.fillStyle = ['#d76b5a', '#d8b45a', '#6a9e6f'][index];
      ctx.beginPath();
      ctx.arc(20 + index * 22, 17, 6, 0, Math.PI * 2);
      ctx.fill();
    }
    ctx.fillStyle = '#1b2730';
    ctx.fillRect(0, 34, 96, 286);
    for (let index = 0; index < 8; index += 1) {
      ctx.fillStyle = index === 1 ? '#557a95' : '#33454f';
      ctx.fillRect(14, 58 + index * 26, 64 - (index % 3) * 10, 8);
    }

    const colors = ['#7fb3d5', '#98c379', '#d8b45a', '#c678dd', '#d19a66', '#abb2bf'];
    const rows = [
      [0, 0.62, 44],
      [0, 0.85, 26],
      [1, 0.4, 70],
      [2, 0.55, 34],
      [2, 0.72, 48],
      [3, 0.35, 58],
      [4, 0.5, 32],
      [4, 0.66, 74],
      [5, 0.28, 40],
      [1, 0.52, 54],
      [2, 0.3, 62],
      [0, 0.44, 30],
      [4, 0.38, 46],
      [5, 0.58, 28],
    ] as const;
    rows.forEach(([colorIndex, indent, width], rowIndex) => {
      ctx.fillStyle = colors[colorIndex];
      ctx.fillRect(118 + indent * 120, 58 + rowIndex * 17, width, 6);
    });

    ctx.fillStyle = '#24343e';
    ctx.fillRect(346, 196, 144, 100);
    ctx.strokeStyle = '#4c7b64';
    ctx.lineWidth = 3;
    ctx.beginPath();
    ctx.moveTo(360, 280);
    ctx.lineTo(392, 252);
    ctx.lineTo(424, 264);
    ctx.lineTo(468, 222);
    ctx.stroke();
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.anisotropy = 4;
  screenTexture = texture;
  return texture;
}

/** Release the shared screen texture. Call from the scene dispose path. */
export function disposeScreenTexture(): void {
  screenTexture?.dispose();
  screenTexture = null;
}

/** A desk with a seated agent, monitor, keyboard, mug and papers. */
export function createDesk(color: string, small: boolean, director = false): DeskVisual {
  const group = new THREE.Group();
  const scale = small ? 0.78 : 1;
  group.scale.setScalar(scale);
  const wood = PALETTE.wood;
  const width = director ? 2.3 : 1.7;
  const depth = director ? 1.25 : 0.95;
  // Rounded, slightly glossy top slab — reads as finished wood rather than a
  // raw box edge.
  const top = new THREE.Mesh(
    new RoundedBoxGeometry(width, 0.09, depth, 3, 0.04),
    new THREE.MeshStandardMaterial({ color: wood, roughness: 0.55, metalness: 0.06 }),
  );
  top.position.set(0, 0.74, 0);
  top.castShadow = true;
  top.receiveShadow = true;
  group.add(top);
  // Slim two-drawer pedestal under the work side and a cable raceway so the
  // legs are not four bare posts.
  const pedestal = box(0.55, 0.62, depth - 0.12, '#cbb99d', -width / 2 + 0.33, 0.38, 0);
  group.add(pedestal);
  for (const drawerY of [0.5, 0.28]) {
    group.add(box(0.42, 0.05, 0.02, PALETTE.woodDark, -width / 2 + 0.33, drawerY, depth / 2 - 0.085));
  }
  for (const x of [width / 2 - 0.1]) {
    for (const z of [-depth / 2 + 0.08, depth / 2 - 0.08]) {
      group.add(box(0.08, 0.72, 0.08, PALETTE.woodDark, x, 0.36, z));
    }
  }

  // Monitor on the far side of the agent, laptop for the director.
  if (director) {
    const laptopBase = box(0.62, 0.03, 0.44, '#2b2721', -0.62, 0.81, 0.05);
    group.add(laptopBase);
    const laptopScreen = box(0.6, 0.36, 0.03, '#2b3c49', -0.62, 0.99, -0.16);
    laptopScreen.rotation.x = -0.28;
    group.add(laptopScreen);
    group.add(box(0.46, 0.04, 0.32, PALETTE.white, 0.55, 0.81, 0.1));
    group.add(box(0.28, 0.06, 0.2, PALETTE.white, 0.55, 0.83, -0.18));
  } else {
    const stand = box(0.2, 0.03, 0.15, '#2b2721', 0, 0.79, -0.22);
    group.add(stand);
    group.add(box(0.07, 0.24, 0.07, '#2b2721', 0, 0.9, -0.22));
    group.add(box(0.7, 0.44, 0.045, '#2b2721', 0, 1.08, -0.22));
    const screen = new THREE.Mesh(
      new THREE.PlaneGeometry(0.64, 0.38),
      new THREE.MeshStandardMaterial({
        map: getScreenTexture(),
        emissiveMap: getScreenTexture(),
        emissive: '#ffffff',
        emissiveIntensity: 0.5,
        roughness: 0.32,
        metalness: 0.05,
      }),
    );
    screen.position.set(0, 1.08, -0.195);
    group.add(screen);
    group.add(box(0.52, 0.03, 0.16, '#3a352e', 0, 0.81, 0.06));
    group.add(box(0.1, 0.03, 0.13, '#3a352e', 0.36, 0.81, 0.08));
  }

  // Mug with a colored drink, papers and a small desk plant.
  const mugColor = director ? PALETTE.white : PALETTE.terracotta;
  const mug = cylinder(0.06, 0.05, 0.12, mugColor, 12);
  mug.position.set(director ? 0.95 : 0.62, 0.86, director ? -0.25 : 0.1);
  group.add(mug);
  const drink = cylinder(0.048, 0.048, 0.015, director ? '#6b4a2f' : '#8d5f49', 12);
  drink.position.set(mug.position.x, 0.923, mug.position.z);
  group.add(drink);
  // Paper stack with a coloured sheet so it reads as a document, not a blank.
  group.add(box(0.34, 0.015, 0.24, PALETTE.white, director ? -0.1 : -0.55, 0.8, 0.18));
  group.add(box(0.3, 0.012, 0.2, '#cfe2ee', director ? -0.1 : -0.55, 0.814, 0.18));
  group.add(box(0.24, 0.005, 0.02, '#557a95', director ? -0.1 : -0.55, 0.822, 0.24));
  const deskPlant = createDeskPlant();
  deskPlant.position.set(director ? 0.95 : 0.7, 0.79, 0.26);
  group.add(deskPlant);

  // Seated person: agents sit on the camera side facing the monitor; the
  // director sits behind the desk facing the camera.
  const person = createSeatedPerson(color, director);
  person.group.position.set(0, 0, director ? -0.62 : 0.56);
  person.group.rotation.y = director ? 0 : Math.PI;
  group.add(person.group);

  const ringMaterial = new THREE.MeshBasicMaterial({ color });
  // A flatter, softer ellipse reads as a floor pad rather than a hard ring.
  const ring = new THREE.Mesh(new THREE.TorusGeometry(0.34, 0.028, 6, 18), ringMaterial);
  ring.rotation.x = Math.PI / 2;
  ring.position.set(0, 0.03, director ? -0.62 : 0.56);
  group.add(ring);
  const pad = new THREE.Mesh(
    new THREE.CircleGeometry(0.33, 18),
    new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0.14, depthWrite: false }),
  );
  pad.rotation.x = -Math.PI / 2;
  pad.position.set(0, 0.035, director ? -0.62 : 0.56);
  group.add(pad);

  const ledMaterial = new THREE.MeshBasicMaterial({ color });
  const led = new THREE.Mesh(new THREE.SphereGeometry(0.02, 8, 6), ledMaterial);
  led.position.set(0.29, 0.92, -0.195);
  group.add(led);

  return {
    group,
    person,
    torsoMaterial: person.torsoMaterial,
    ringMaterial,
    ledMaterial,
    small,
  };
}

/** A small desk succulent: five pointed leaves in a terracotta pot. */
export function createDeskPlant(): THREE.Group {
  const group = new THREE.Group();
  const pot = cylinder(0.085, 0.065, 0.13, PALETTE.terracotta, 10);
  pot.position.y = 0.065;
  group.add(pot);
  const soil = cylinder(0.08, 0.08, 0.015, '#4a3127', 10);
  soil.position.y = 0.135;
  group.add(soil);
  const leaves = [
    [0, 0, 0, 0, 1],
    [-0.06, 0.015, 0, 0.28, 0.9],
    [0.06, 0.01, 0, -0.32, 0.85],
    [0.01, -0.05, -0.26, 0, 0.8],
    [-0.01, 0.05, 0.3, 0.04, 0.75],
  ] as const;
  for (const [x, z, tiltX, tiltZ, scale] of leaves) {
    const leaf = new THREE.Mesh(
      new THREE.ConeGeometry(0.036, 0.26, 6),
      new THREE.MeshStandardMaterial({
        color: scale > 0.85 ? '#4f7d45' : '#3f6b3a',
        roughness: 0.68,
      }),
    );
    leaf.position.set(x, 0.13 + 0.13, z);
    leaf.rotation.set(tiltX, 0, tiltZ);
    leaf.scale.set(1, scale, 0.55);
    leaf.castShadow = true;
    group.add(leaf);
  }
  return group;
}

/** A potted plant with layered, slightly glossy leaves. */
export function createPlant(scale = 1): THREE.Group {
  const group = new THREE.Group();
  const pot = cylinder(0.2 * scale, 0.145 * scale, 0.3 * scale, PALETTE.terracotta, 16);
  pot.position.y = 0.15 * scale;
  group.add(pot);
  const rim = cylinder(0.215 * scale, 0.215 * scale, 0.05 * scale, '#bd8a6c', 16);
  rim.position.y = 0.3 * scale;
  group.add(rim);
  const soil = cylinder(0.185 * scale, 0.185 * scale, 0.02 * scale, '#4a3127', 16);
  soil.position.y = 0.325 * scale;
  group.add(soil);

  const greens = ['#3f6b3a', '#4f7d45', '#5f8d52', '#35603a'];
  const leaves = [
    [0.0, 1.12, 0.02, 0.15, 0.0, 1.0, 0],
    [-0.3, 0.92, 0.08, 0.45, 0.25, 0.85, 1],
    [0.31, 0.98, -0.06, -0.5, -0.2, 0.9, 2],
    [0.06, 0.86, 0.28, -0.3, 0.55, 0.75, 3],
    [-0.18, 1.2, -0.16, 0.3, -0.45, 0.8, 2],
    [0.22, 1.28, 0.16, -0.15, 0.3, 0.7, 1],
  ] as const;

  for (const [x, y, z, tiltX, tiltZ, size, colorIndex] of leaves) {
    const from = new THREE.Vector3(0, 0.33 * scale, 0);
    const to = new THREE.Vector3(x * scale, y * scale - 0.06, z * scale);
    const direction = new THREE.Vector3().subVectors(to, from);
    const length = direction.length();
    const stem = cylinder(0.011 * scale, 0.015 * scale, length, '#5d5a3a', 6);
    stem.position.copy(from).addScaledVector(direction, 0.5);
    stem.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), direction.normalize());
    group.add(stem);

    const leaf = new THREE.Mesh(
      new THREE.SphereGeometry(0.22, 10, 7),
      new THREE.MeshStandardMaterial({
        color: greens[colorIndex],
        roughness: 0.62,
        metalness: 0.02,
      }),
    );
    leaf.scale.set(1.45 * size, 0.16, 0.82 * size);
    leaf.position.set(x * scale, y * scale, z * scale);
    leaf.rotation.set(tiltX, 0, tiltZ);
    leaf.castShadow = true;
    group.add(leaf);
    const vein = box(
      0.012 * scale,
      0.015,
      0.4 * size * scale,
      '#c7d6b8',
      x * scale,
      y * scale + 0.015,
      z * scale,
    );
    vein.rotation.set(tiltX, 0, tiltZ);
    group.add(vein);
  }
  return group;
}

/** Office water cooler with two taps and a cup stack. */
export function createDispenser(): THREE.Group {
  const group = new THREE.Group();
  // Rounded body with a glossy front panel, taps and a tray.
  const base = new THREE.Mesh(
    new RoundedBoxGeometry(0.55, 0.95, 0.48, 3, 0.05),
    new THREE.MeshStandardMaterial({ color: '#e4e0d8', roughness: 0.45, metalness: 0.08 }),
  );
  base.position.set(0, 0.48, 0);
  base.castShadow = true;
  base.receiveShadow = true;
  group.add(base);
  group.add(box(0.42, 0.36, 0.03, '#b7b2a6', 0, 0.62, 0.25));
  for (const x of [-0.1, 0.1]) {
    const tap = cylinder(0.02, 0.02, 0.08, PALETTE.blue, 8);
    tap.rotation.x = Math.PI / 2;
    tap.position.set(x, 0.7, 0.29);
    group.add(tap);
  }
  const tray = box(0.36, 0.02, 0.16, '#b7b2a6', 0, 0.52, 0.31);
  group.add(tray);
  const bottle = new THREE.Mesh(
    new THREE.CylinderGeometry(0.17, 0.13, 0.52, 14),
    new THREE.MeshStandardMaterial({
      color: '#a9d5e8',
      transparent: true,
      opacity: 0.7,
      roughness: 0.2,
    }),
  );
  bottle.position.set(0, 1.2, 0);
  bottle.castShadow = true;
  group.add(bottle);
  const cap = cylinder(0.07, 0.07, 0.06, PALETTE.blue, 10);
  cap.position.set(0, 1.49, 0);
  group.add(cap);
  for (let index = 0; index < 3; index += 1) {
    const cup = cylinder(0.035, 0.03, 0.07, PALETTE.white, 10);
    cup.position.set(0.32, 0.72 + index * 0.07, 0.12);
    group.add(cup);
  }
  return group;
}

/** Freestanding whiteboard with a drawn diagram. */
export function createWhiteboard(): THREE.Group {
  const group = new THREE.Group();
  const width = 3.0;
  const height = 1.65;
  // Thin aluminium frame around the board, then the legs.
  const board = new THREE.Mesh(
    new RoundedBoxGeometry(width, height, 0.045, 2, 0.03),
    new THREE.MeshStandardMaterial({ color: '#d7d2c8', roughness: 0.35, metalness: 0.55 }),
  );
  board.position.set(0, 1.55, 0);
  board.castShadow = true;
  board.receiveShadow = true;
  group.add(board);
  group.add(box(width + 0.1, 0.05, 0.08, '#b7b2a6', 0, 2.4, 0));
  for (const x of [-width / 2 + 0.2, width / 2 - 0.2]) {
    group.add(box(0.07, 2.4, 0.07, '#8d8578', x, 1.2, -0.05));
  }

  const canvas = document.createElement('canvas');
  canvas.width = 512;
  canvas.height = 256;
  const ctx = canvas.getContext('2d');
  if (ctx) {
    ctx.fillStyle = '#fbf9f4';
    ctx.fillRect(0, 0, 512, 256);
    ctx.strokeStyle = '#3f5b70';
    ctx.lineWidth = 4;
    ctx.strokeRect(40, 48, 150, 80);
    ctx.strokeRect(320, 48, 150, 80);
    ctx.strokeStyle = '#a8775f';
    ctx.beginPath();
    ctx.moveTo(190, 88);
    ctx.lineTo(320, 88);
    ctx.stroke();
    ctx.strokeStyle = '#3f5d3a';
    ctx.beginPath();
    ctx.moveTo(60, 190);
    ctx.lineTo(200, 160);
    ctx.lineTo(300, 200);
    ctx.lineTo(450, 170);
    ctx.stroke();
    ctx.fillStyle = '#5a5a52';
    ctx.font = '600 18px system-ui, sans-serif';
    ctx.fillText('Roadmap', 62, 38);
    ctx.fillText('Q4', 72, 200);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  const drawing = new THREE.Mesh(
    new THREE.PlaneGeometry(width - 0.14, height - 0.14),
    new THREE.MeshBasicMaterial({ map: texture }),
  );
  drawing.position.set(0, 1.55, 0.03);
  group.add(drawing);
  return group;
}

/** Framed picture with an abstract print. */
export function createPicture(color: string): THREE.Group {
  const group = new THREE.Group();
  group.add(box(0.95, 0.72, 0.05, PALETTE.woodDark, 0, 0, 0));
  // A small palette-tinted matte so the print floats inside the frame.
  group.add(box(0.85, 0.62, 0.012, PALETTE.wall, 0, 0, 0.028));
  const canvas = document.createElement('canvas');
  canvas.width = 256;
  canvas.height = 192;
  const ctx = canvas.getContext('2d');
  if (ctx) {
    ctx.fillStyle = '#f4f1ec';
    ctx.fillRect(0, 0, 256, 192);
    ctx.fillStyle = color;
    ctx.beginPath();
    ctx.arc(170, 80, 54, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = '#d2c4ae';
    ctx.fillRect(34, 110, 130, 48);
    ctx.fillStyle = '#3f5d3a';
    ctx.fillRect(54, 58, 32, 100);
  }
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  const print = new THREE.Mesh(
    new THREE.PlaneGeometry(0.82, 0.59),
    new THREE.MeshBasicMaterial({ map: texture }),
  );
  print.position.z = 0.034;
  group.add(print);
  return group;
}

/** A low glass partition with a top rail. */
export function createGlassPartition(length: number, height = 1.15): THREE.Group {
  const group = new THREE.Group();
  const pane = new THREE.Mesh(
    new THREE.BoxGeometry(length, height - 0.12, 0.05),
    new THREE.MeshStandardMaterial({
      color: PALETTE.glass,
      transparent: true,
      opacity: 0.28,
      roughness: 0.15,
      metalness: 0.05,
      side: THREE.DoubleSide,
    }),
  );
  pane.position.y = (height - 0.12) / 2 + 0.06;
  group.add(pane);
  group.add(box(length, 0.08, 0.1, PALETTE.wallTrim, 0, height, 0));
  group.add(box(length, 0.08, 0.1, PALETTE.wallTrim, 0, 0.04, 0));
  for (const x of [-length / 2 + 0.05, length / 2 - 0.05]) {
    group.add(box(0.08, height, 0.1, PALETTE.wallTrim, x, height / 2, 0));
  }
  return group;
}

/** A low solid partition with a pale top cap. */
export function createLowWall(length: number, height = 1.05): THREE.Group {
  const group = new THREE.Group();
  group.add(box(length, height, 0.22, PALETTE.wall, 0, height / 2, 0));
  group.add(box(length + 0.06, 0.06, 0.28, PALETTE.wallTrim, 0, height + 0.03, 0));
  group.add(box(length, 0.08, 0.24, PALETTE.wallTrim, 0, 0.04, 0));
  return group;
}

/** Exterior wall with a window band and mullions, along local X. */
export function createWindowWall(length: number): THREE.Group {
  const group = new THREE.Group();
  group.add(box(length, 0.85, 0.22, PALETTE.wall, 0, 0.425, 0));
  group.add(box(length, 0.5, 0.22, PALETTE.wall, 0, 2.55, 0));
  group.add(box(length, 0.06, 0.26, PALETTE.wallTrim, 0, 0.88, 0));
  group.add(box(length, 0.06, 0.26, PALETTE.wallTrim, 0, 2.28, 0));
  const pane = new THREE.Mesh(
    new THREE.BoxGeometry(length, 1.34, 0.05),
    new THREE.MeshStandardMaterial({
      color: '#a9cfe4',
      emissive: '#cfe3ee',
      emissiveIntensity: 0.08,
      transparent: true,
      opacity: 0.38,
      roughness: 0.1,
      metalness: 0,
    }),
  );
  pane.position.y = 1.58;
  group.add(pane);
  const count = Math.max(2, Math.round(length / 2.6));
  for (let index = 0; index <= count; index += 1) {
    const x = -length / 2 + (length / count) * index;
    group.add(box(0.09, 1.4, 0.12, PALETTE.wallTrim, x, 1.58, 0));
  }
  return group;
}

/** A low-poly figure that looks like it is seated at a table. */
export function createSittingFigure(
  color: string,
  position: [number, number, number],
  rotationY = 0,
  faceVisible = true,
): THREE.Group {
  const group = new THREE.Group();
  const shirt = material(color, 0.82);
  const pants = material('#3a3f45', 0.9);
  const skin = material('#d5b9a3', 0.85);
  const hair = material('#4a3127', 0.85);

  const hips = box(0.34, 0.18, 0.32, '#3a3f45', 0, 0.5, 0);
  group.add(hips);
  const torso = new THREE.Mesh(new THREE.CapsuleGeometry(0.18, 0.24, 4, 10), shirt);
  torso.scale.z = 0.8;
  torso.position.set(0, 0.86, 0.02);
  torso.castShadow = true;
  group.add(torso);
  const neck = cylinder(0.05, 0.055, 0.09, '#d5b9a3', 8);
  neck.position.set(0, 1.13, 0.03);
  group.add(neck);
  const head = sphere(0.155, '#d5b9a3', 12);
  head.position.set(0, 1.28, 0.03);
  group.add(head);
  const hairMesh = sphere(0.162, '#4a3127', 10);
  hairMesh.scale.set(1, 0.6, 1);
  hairMesh.position.set(0, 1.36, 0.0);
  group.add(hairMesh);
  for (const side of [-1, 1]) {
    const eye = sphere(0.015, '#2b2721', 6);
    eye.position.set(side * 0.05, 1.3, 0.16);
    if (faceVisible) group.add(eye);
  }

  // Arms reaching forward onto the table.
  for (const side of [-1, 1]) {
    const arm = capsule(0.05, 0.14, color, 8);
    arm.position.set(side * 0.21, 0.78, 0.18);
    arm.rotation.x = -1.0;
    group.add(arm);
    const hand = sphere(0.05, '#d5b9a3', 8);
    hand.position.set(side * 0.21, 0.74, 0.34);
    group.add(hand);
  }
  // Legs: thighs forward, shins down, shoes on the floor.
  for (const side of [-1, 1]) {
    const thigh = capsule(0.07, 0.2, '#3a3f45', 8);
    thigh.rotation.x = Math.PI / 2;
    thigh.position.set(side * 0.1, 0.48, 0.16);
    group.add(thigh);
    const shin = capsule(0.055, 0.24, '#3a3f45', 8);
    shin.position.set(side * 0.1, 0.25, 0.3);
    group.add(shin);
    group.add(box(0.11, 0.07, 0.22, '#2b2721', side * 0.1, 0.05, 0.34));
  }

  group.position.set(position[0], position[1], position[2]);
  group.rotation.y = rotationY;
  return group;
}

/** Meeting table with four chairs. */
export function createMeetingTable(): THREE.Group {
  const group = new THREE.Group();
  // Rounded oak top with a darker under-panel; the old four square posts read
  // as a card table at office scale.
  const top = new THREE.Mesh(
    new RoundedBoxGeometry(3.8, 0.12, 1.4, 3, 0.04),
    new THREE.MeshStandardMaterial({ color: PALETTE.wood, roughness: 0.5, metalness: 0.05 }),
  );
  top.position.set(0, 0.74, 0);
  top.castShadow = true;
  top.receiveShadow = true;
  group.add(top);
  group.add(box(3.4, 0.06, 1.1, '#5d3d30', 0, 0.66, 0));
  for (const x of [-1.6, 1.6]) {
    group.add(box(0.12, 0.64, 1.0, PALETTE.woodDark, x, 0.32, 0));
  }
  // Glazed side of the table, not a black cube.
  group.add(box(0.5, 0.03, 0.36, '#2b2721', -0.9, 0.81, 0.1));
  group.add(box(0.34, 0.02, 0.24, PALETTE.white, 0.7, 0.81, -0.15));
  // A monitor in the middle, papers and a snack plate.
  group.add(box(0.42, 0.28, 0.03, '#2b2721', 0.35, 0.96, -0.18));
  group.add(box(0.38, 0.24, 0.015, '#3f5b70', 0.35, 0.96, -0.165));
  const snack = cylinder(0.11, 0.09, 0.03, '#fffaf2', 12);
  snack.position.set(-0.1, 0.8, 0.28);
  group.add(snack);
  const cup = cylinder(0.04, 0.035, 0.08, '#c98f73', 10);
  cup.position.set(0.1, 0.84, 0.25);
  group.add(cup);

  const seatColors = ['#6f7f8f', '#8d6e63', '#7c8f74', '#a8775f'];
  const seats = [
    [-1.1, 1.05, Math.PI],
    [0.2, 1.05, Math.PI],
    [-1.1, -1.05, 0],
    [0.2, -1.05, 0],
  ] as const;
  const figures = [
    createSittingFigure(seatColors[0], [-1.1, 0.04, 1.05], Math.PI),
    createSittingFigure(seatColors[1], [0.2, 0.04, 1.05], Math.PI),
    createSittingFigure(seatColors[2], [-1.1, 0.04, -1.05], 0),
    createSittingFigure(seatColors[3], [0.2, 0.04, -1.05], 0),
  ];
  for (const [index, [x, z]] of seats.entries()) {
    group.add(box(0.42, 0.08, 0.42, '#3a352e', x, 0.44, z));
    group.add(box(0.42, 0.45, 0.07, '#3a352e', x, 0.7, z + (z > 0 ? 0.18 : -0.18)));
    group.add(figures[index]);
  }
  return group;
}

/** Small lounge sofa with two seated people and a throw pillow. */
export function createSofa(): THREE.Group {
  const group = new THREE.Group();
  // Soft fabric upholstery: a deeper terracotta seat, cushions and a low
  // wooden frame so the sofa reads as textile rather than stacked boxes.
  group.add(box(1.9, 0.3, 0.78, PALETTE.terracotta, 0, 0.3, 0));
  group.add(box(1.9, 0.44, 0.18, PALETTE.terracotta, 0, 0.58, -0.3));
  group.add(box(0.16, 0.42, 0.78, PALETTE.terracotta, -0.87, 0.48, 0));
  group.add(box(0.16, 0.42, 0.78, PALETTE.terracotta, 0.87, 0.48, 0));
  group.add(box(1.7, 0.07, 0.7, PALETTE.woodDark, 0, 0.11, 0.02));
  group.add(box(0.7, 0.06, 0.42, '#5d3d30', 0, 0.09, 0.55));
  group.add(box(0.46, 0.26, 0.03, PALETTE.white, -0.52, 0.52, 0.08));
  group.add(box(0.4, 0.24, 0.03, '#cfe2ee', 0.55, 0.52, 0.08));

  // Two people sunk into the sofa, facing the coffee table.
  for (const [index, x] of [-0.42, 0.42].entries()) {
    group.add(createSittingFigure(index === 0 ? '#557a95' : '#8d6e63', [x, 0.2, 0.05], 0));
  }
  // Seat cushions.
  group.add(box(0.8, 0.09, 0.62, '#bd9480', -0.44, 0.49, 0.04));
  group.add(box(0.8, 0.09, 0.62, '#bd9480', 0.44, 0.49, 0.04));
  group.add(box(0.38, 0.12, 0.38, '#d9cbb8', 0, 0.42, 0.05));
  return group;
}

/** The operator's desk: a suit, a nameplate and a view of the room. */
export function createDirectorDesk(): THREE.Group {
  const group = createDesk(PALETTE.blue, false, true).group;
  const label = makeLabel('YOU · DIRECTOR', 0.5);
  label.position.set(0, 1.68, 0.38);
  group.add(label);
  return group;
}
