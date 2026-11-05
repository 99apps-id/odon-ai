// Level, physics, collision and rendering for the 2D platformer.
// No dependencies: this is ordinary canvas 2D.

export const WIDTH = 960;
export const HEIGHT = 540;

const PLATFORMS = [
  { x: 0, y: 500, w: 960, h: 40 },
  { x: 180, y: 410, w: 150, h: 18 },
  { x: 380, y: 340, w: 130, h: 18 },
  { x: 560, y: 270, w: 150, h: 18 },
  { x: 750, y: 200, w: 150, h: 18 },
];

const COIN_LAYOUT = [
  { x: 255, y: 370 },
  { x: 445, y: 300 },
  { x: 635, y: 230 },
  { x: 825, y: 160 },
];

const GOAL = { x: 850, y: 140, w: 38, h: 60 };
const SPAWN = { x: 70, y: 458, w: 28, h: 36 };

const MOVE_SPEED = 300;
const ACCEL = 2400;
const FRICTION = 2600;
const GRAVITY = 1900;
const JUMP_SPEED = 700;
const COIN_RADIUS = 26;

const REDUCE_MOTION =
  typeof window !== 'undefined' &&
  window.matchMedia &&
  window.matchMedia('(prefers-reduced-motion: reduce)').matches;

export function createState() {
  return {
    player: { ...SPAWN, vx: 0, vy: 0, onGround: false, facing: 1 },
    coins: COIN_LAYOUT.map((coin, index) => ({
      ...coin,
      id: index,
      taken: false,
      phase: index * 0.8,
    })),
    elapsed: 0,
    status: 'playing',
    hint: '',
    hintTimer: 0,
  };
}

function overlap(a, b) {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
}

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function respawn(player) {
  Object.assign(player, { ...SPAWN, vx: 0, vy: 0, onGround: false });
}

export function step(state, input, dt) {
  if (state.status !== 'playing') return;

  state.elapsed += dt;
  const player = state.player;

  const target = input.axis * MOVE_SPEED;
  const rate = input.axis === 0 ? FRICTION : ACCEL;
  if (player.vx < target) player.vx = Math.min(player.vx + rate * dt, target);
  else if (player.vx > target) player.vx = Math.max(player.vx - rate * dt, target);
  if (input.axis !== 0) player.facing = input.axis;

  if (input.jump && player.onGround) {
    player.vy = -JUMP_SPEED;
    player.onGround = false;
  }

  player.vy += GRAVITY * dt;

  // Horizontal movement, then resolve against platforms.
  player.x += player.vx * dt;
  for (const platform of PLATFORMS) {
    if (!overlap(player, platform)) continue;
    if (player.vx > 0) player.x = platform.x - player.w;
    else if (player.vx < 0) player.x = platform.x + platform.w;
    player.vx = 0;
  }
  player.x = clamp(player.x, 0, WIDTH - player.w);

  // Vertical movement: land on the first platform crossed this step.
  player.y += player.vy * dt;
  player.onGround = false;
  for (const platform of PLATFORMS) {
    if (!overlap(player, platform)) continue;
    if (player.vy > 0) {
      player.y = platform.y - player.h;
      player.vy = 0;
      player.onGround = true;
    } else if (player.vy < 0) {
      player.y = platform.y + platform.h;
      player.vy = 0;
    }
  }

  if (player.y > HEIGHT + 160) respawn(player);

  let collected = 0;
  for (const coin of state.coins) {
    if (coin.taken) {
      collected += 1;
      continue;
    }
    const dx = player.x + player.w / 2 - coin.x;
    const dy = player.y + player.h / 2 - coin.y;
    if (dx * dx + dy * dy < COIN_RADIUS * COIN_RADIUS) {
      coin.taken = true;
      collected += 1;
    }
  }

  if (overlap(player, GOAL)) {
    if (collected === state.coins.length) {
      state.status = 'won';
    } else if (state.hintTimer <= 0) {
      state.hint = `Collect every coin first (${collected}/${state.coins.length}).`;
      state.hintTimer = 2.2;
    }
  }
  if (state.hintTimer > 0) {
    state.hintTimer -= dt;
    if (state.hintTimer <= 0) state.hint = '';
  }
}

function fillRound(ctx, x, y, w, h, radius, color) {
  ctx.fillStyle = color;
  ctx.beginPath();
  if (ctx.roundRect) ctx.roundRect(x, y, w, h, radius);
  else ctx.rect(x, y, w, h);
  ctx.fill();
}

export function render(ctx, state) {
  const background = ctx.createLinearGradient(0, 0, 0, HEIGHT);
  background.addColorStop(0, '#faf8f5');
  background.addColorStop(1, '#efe6d8');
  ctx.fillStyle = background;
  ctx.fillRect(0, 0, WIDTH, HEIGHT);

  // Soft dots so the empty sky reads as a designed scene.
  ctx.fillStyle = 'rgba(183, 166, 140, 0.18)';
  for (let row = 0; row < 7; row += 1) {
    for (let column = 0; column < 13; column += 1) {
      ctx.beginPath();
      ctx.arc(40 + column * 74, 40 + row * 68, 1.6, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  for (const platform of PLATFORMS) {
    fillRound(ctx, platform.x, platform.y, platform.w, platform.h, 7, '#d2c4ae');
    fillRound(ctx, platform.x, platform.y, platform.w, 5, 3, '#e5dccd');
  }

  // Goal flag.
  ctx.fillStyle = '#4c463d';
  ctx.fillRect(GOAL.x + 6, GOAL.y, 4, GOAL.h);
  ctx.fillStyle = '#047857';
  ctx.beginPath();
  ctx.moveTo(GOAL.x + 10, GOAL.y + 4);
  ctx.lineTo(GOAL.x + 10, GOAL.y + 26);
  ctx.lineTo(GOAL.x + 36, GOAL.y + 15);
  ctx.closePath();
  ctx.fill();

  for (const coin of state.coins) {
    if (coin.taken) continue;
    const bob = REDUCE_MOTION ? 0 : Math.sin(state.elapsed * 3 + coin.phase) * 5;
    ctx.fillStyle = '#b45309';
    ctx.beginPath();
    ctx.arc(coin.x, coin.y + bob, 10, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = '#f3c96b';
    ctx.beginPath();
    ctx.arc(coin.x - 2, coin.y - 2 + bob, 6, 0, Math.PI * 2);
    ctx.fill();
  }

  const player = state.player;
  ctx.fillStyle = 'rgba(34, 31, 28, 0.10)';
  ctx.beginPath();
  ctx.ellipse(player.x + player.w / 2, player.y + player.h + 4, player.w / 2, 5, 0, 0, Math.PI * 2);
  ctx.fill();
  fillRound(ctx, player.x, player.y, player.w, player.h, 8, '#3f5b70');
  fillRound(ctx, player.x + 4, player.y + 6, player.w - 8, 12, 5, '#557a95');
  ctx.fillStyle = '#fffaf2';
  const eyeX = player.facing >= 0 ? player.x + player.w - 12 : player.x + 8;
  ctx.beginPath();
  ctx.arc(eyeX, player.y + 12, 2.4, 0, Math.PI * 2);
  ctx.fill();

  if (state.hint) {
    ctx.font = '600 16px ui-sans-serif, system-ui, sans-serif';
    ctx.textAlign = 'center';
    const width = ctx.measureText(state.hint).width + 28;
    fillRound(ctx, (WIDTH - width) / 2, 24, width, 34, 17, 'rgba(34, 31, 28, 0.82)');
    ctx.fillStyle = '#fffaf2';
    ctx.fillText(state.hint, WIDTH / 2, 46);
    ctx.textAlign = 'start';
  }
}
