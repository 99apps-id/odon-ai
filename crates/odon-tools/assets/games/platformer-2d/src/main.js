import { createState, render, step } from './game.js';

const canvas = document.getElementById('game');
const context = canvas.getContext('2d');
const coinsLabel = document.getElementById('coins');
const timerLabel = document.getElementById('timer');
const overlay = document.getElementById('overlay');

const LEFT = new Set(['ArrowLeft', 'KeyA']);
const RIGHT = new Set(['ArrowRight', 'KeyD']);
const JUMP = new Set(['ArrowUp', 'KeyW', 'Space']);

const keys = new Set();
let state = createState();
let jumpQueued = false;
let overlayShown = false;
let last = performance.now();

window.addEventListener('keydown', (event) => {
  if (LEFT.has(event.code) || RIGHT.has(event.code) || JUMP.has(event.code)) {
    event.preventDefault();
  }
  if (event.repeat) return;
  keys.add(event.code);
  if (JUMP.has(event.code)) jumpQueued = true;
  if (event.code === 'KeyR') restart();
});

window.addEventListener('keyup', (event) => {
  keys.delete(event.code);
});

window.addEventListener('blur', () => {
  keys.clear();
});

function axis() {
  let value = 0;
  for (const code of keys) {
    if (LEFT.has(code)) value -= 1;
    if (RIGHT.has(code)) value += 1;
  }
  return Math.max(-1, Math.min(1, value));
}

function restart() {
  state = createState();
  overlay.classList.add('hidden');
  overlay.replaceChildren();
  overlayShown = false;
}

function showWin() {
  overlay.replaceChildren();
  const panel = document.createElement('div');
  const title = document.createElement('h2');
  title.textContent = 'Level clear';
  const detail = document.createElement('p');
  detail.textContent = `${state.coins.length} coins · ${state.elapsed.toFixed(1)}s`;
  const button = document.createElement('button');
  button.type = 'button';
  button.textContent = 'Play again';
  button.addEventListener('click', restart);
  panel.append(title, detail, button);
  overlay.append(panel);
  overlay.classList.remove('hidden');
}

function updateHud() {
  const taken = state.coins.filter((coin) => coin.taken).length;
  coinsLabel.textContent = `Coins ${taken}/${state.coins.length}`;
  timerLabel.textContent = `${state.elapsed.toFixed(1)}s`;
}

function frame(now) {
  const dt = Math.min((now - last) / 1000, 0.033);
  last = now;

  if (state.status === 'playing') {
    step(state, { axis: axis(), jump: jumpQueued }, dt);
  }
  jumpQueued = false;

  render(context, state);
  updateHud();

  if (state.status === 'won' && !overlayShown) {
    overlayShown = true;
    showWin();
  }

  requestAnimationFrame(frame);
}

requestAnimationFrame(frame);
