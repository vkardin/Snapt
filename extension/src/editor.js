// The editor: annotates a screenshot with Snapt's own drawing code, compiled to
// WebAssembly from the desktop app's sources (see extension/wasm), then copies
// or saves it.

const t = (key, ...subs) => chrome.i18n.getMessage(key, subs);

// The desktop app's tools in its toolbar order, with its shortcut keys.
const TOOLS = [
  { key: 'p', name: 'toolPen', icon: '<path d="M4 20c4-1 6-3 9-7l4-4a2 2 0 0 0-3-3l-4 4c-4 3-6 5-6 10z"/>' },
  { key: 'l', name: 'toolLine', icon: '<path d="M5 19 19 5"/>' },
  { key: 'a', name: 'toolArrow', icon: '<path d="M5 19 19 5M10 5h9v9"/>' },
  { key: 'r', name: 'toolRect', icon: '<rect x="4" y="6" width="16" height="12" rx="1"/>' },
  { key: 'e', name: 'toolEllipse', icon: '<ellipse cx="12" cy="12" rx="8" ry="6"/>' },
  { key: 'm', name: 'toolMarker', icon: '<path d="m9 15 7-11 4 3-7 11zM9 15l-2 5h4"/>' },
  { key: 't', name: 'toolText', icon: '<path d="M5 6V4h14v2M12 4v16M9 20h6"/>' },
  { key: 'n', name: 'toolNumber', icon: '<circle cx="12" cy="12" r="8"/><path d="M11 9l2-1v8"/>' },
  { key: 'b', name: 'toolPixelate', icon: '<path d="M4 4h5v5H4zM15 4h5v5h-5zM9.5 9.5h5v5h-5zM4 15h5v5H4zM15 15h5v5h-5z"/>' },
  { key: 'u', name: 'toolBlur', icon: '<path d="M12 3c3 4 6 7 6 11a6 6 0 0 1-12 0c0-4 3-7 6-11z"/><path d="M9 14a3 3 0 0 0 3 3"/>' },
];
const TEXT = 6;
const NUMBER = 7;
const HIDES = new Set([8, 9]);
const PALETTE = [0xff3b30, 0xff9500, 0xffcc00, 0x34c759, 0x32ade6, 0x007aff, 0xaf52de, 0xff2d78, 0x000000, 0xffffff];
const STROKES = [1, 30];
const TEXT_SIZES = [10, 96];

const $ = (selector) => document.querySelector(selector);
const view = $('.view');
const ctx = view.getContext('2d');
const outline = $('.outline');
const caretEl = $('.caret');
const typing = $('.typing');
const sizeInput = $('.size input');
const sizeOutput = $('.size output');

const style = { tool: 2, color: PALETTE[0], stroke: 3, text: 20, fill: false };
let wasm;
let scale = 1;
let selected = -1;
let drag = null;
let editing = false;
// Blur and pixelation read the pixels around them, so renders of a part of the
// image take a wider margin once either is on it.
let hides = false;
let dirty = null;
let frame = 0;

main().catch((err) => {
  console.error(err);
  $('.status').textContent = t('captureFailed', err.message || String(err));
});

async function main() {
  for (const el of document.querySelectorAll('[data-i18n]')) el.textContent = t(el.dataset.i18n);
  const id = location.hash.slice(1);
  const [shot, module] = await Promise.all([
    id === 'screen' ? null : chrome.runtime.sendMessage({ type: 'take', id }),
    WebAssembly.instantiateStreaming(fetch('snapt.wasm')),
  ]);
  wasm = module.instance.exports;
  let image;
  if (id === 'screen') {
    image = await pickScreen();
  } else if (shot) {
    document.title = shot.title ? `${shot.title} · Snapt` : 'Snapt';
    image = await compose(shot);
  } else {
    $('.status').textContent = t('captureGone');
    return;
  }
  load(image);
  document.querySelector('.status')?.remove();
  buildToolbar();
  bindInput();
  document.body.classList.add('ready');
}

// A screen or a window the user picks in the browser's sharing dialog. Browsers
// only open that dialog on a click, so the page asks for one first; sharing
// stops as soon as one frame is taken.
async function pickScreen() {
  const status = $('.status');
  status.classList.add('screen');
  status.innerHTML = '<span></span><button class="wide primary"></button>';
  status.firstChild.textContent = t('screenHint');
  const button = status.lastChild;
  button.textContent = t('screenButton');
  for (;;) {
    await new Promise((resolve) => button.addEventListener('click', resolve, { once: true }));
    let stream;
    try {
      stream = await navigator.mediaDevices.getDisplayMedia({
        video: { displaySurface: 'monitor' },
        audio: false,
        selfBrowserSurface: 'exclude',
      });
    } catch {
      status.firstChild.textContent = t('screenCancelled');
      continue;
    }
    try {
      const video = document.createElement('video');
      video.muted = true;
      video.srcObject = stream;
      await video.play();
      // The first frame can still be black while the capture starts.
      await new Promise((resolve) => setTimeout(resolve, 150));
      const canvas = new OffscreenCanvas(video.videoWidth, video.videoHeight);
      const c = canvas.getContext('2d');
      c.drawImage(video, 0, 0);
      scale = devicePixelRatio || 1;
      return c.getImageData(0, 0, canvas.width, canvas.height);
    } finally {
      for (const track of stream.getTracks()) track.stop();
    }
  }
}

// Glues the frames of a capture into one image and crops it.
async function compose(shot) {
  const bitmaps = await Promise.all(
    shot.frames.map(async (f) => createImageBitmap(await (await fetch(f.image)).blob())),
  );
  scale = bitmaps[0].width / shot.viewport.width;
  const full = {
    width: bitmaps[0].width,
    height: shot.total ? Math.round(shot.total * scale) : bitmaps[0].height,
  };
  const crop = shot.crop
    ? {
      x: Math.round(shot.crop.x * scale),
      y: Math.round(shot.crop.y * scale),
      width: Math.round(shot.crop.width * scale),
      height: Math.round(shot.crop.height * scale),
    }
    : { x: 0, y: 0, ...full };
  crop.width = Math.max(1, Math.min(crop.width, full.width - crop.x));
  crop.height = Math.max(1, Math.min(crop.height, full.height - crop.y));
  const canvas = new OffscreenCanvas(crop.width, crop.height);
  const c = canvas.getContext('2d');
  shot.frames.forEach((f, i) => c.drawImage(bitmaps[i], -crop.x, Math.round(f.y * scale) - crop.y));
  return c.getImageData(0, 0, crop.width, crop.height);
}

function load(image) {
  const ptr = wasm.alloc(image.data.length);
  new Uint8Array(wasm.memory.buffer, ptr, image.data.length).set(image.data);
  wasm.load(ptr, image.width, image.height);
  view.width = image.width;
  view.height = image.height;
  // One image pixel per screen pixel, unless the window is narrower.
  view.style.width = `${image.width / devicePixelRatio}px`;
  invalidate(null);
}

// ---- Rendering ---------------------------------------------------------------

const full = () => ({ l: 0, t: 0, r: view.width, b: view.height });

function union(a, b) {
  if (!a) return b;
  if (!b) return a;
  return { l: Math.min(a.l, b.l), t: Math.min(a.t, b.t), r: Math.max(a.r, b.r), b: Math.max(a.b, b.b) };
}

// Marks an area of the image, or all of it for null, to be drawn next frame.
function invalidate(area) {
  dirty = area ? union(dirty, area) : full();
  frame ||= requestAnimationFrame(flush);
}

function flush() {
  frame = 0;
  if (!dirty) return;
  const area = dirty;
  dirty = null;
  const l = Math.max(0, Math.floor(area.l));
  const tp = Math.max(0, Math.floor(area.t));
  const r = Math.min(view.width, Math.ceil(area.r));
  const b = Math.min(view.height, Math.ceil(area.b));
  if (r <= l || b <= tp) return;
  const pad = hides ? Math.ceil(200 * scale) : 0;
  const x = Math.max(0, l - pad);
  const y = Math.max(0, tp - pad);
  const w = Math.min(view.width, r + pad) - x;
  const h = Math.min(view.height, b + pad) - y;
  const ptr = wasm.render(x, y, w, h);
  const pixels = new Uint8ClampedArray(wasm.memory.buffer, ptr, w * h * 4);
  ctx.putImageData(new ImageData(pixels, w, h), x, y, l - x, tp - y, r - l, b - tp);
  updateOverlays();
}

const rect = new Float32Array(4);

function bounds(i) {
  if (!wasm.bounds(i)) return null;
  rect.set(new Float32Array(wasm.memory.buffer, wasm.rect_ptr(), 4));
  return { l: rect[0], t: rect[1], r: rect[2], b: rect[3] };
}

// The selection outline and the text caret, in CSS pixels over the canvas.
function updateOverlays() {
  const k = view.clientWidth / view.width;
  const box = selected >= 0 ? bounds(selected) : null;
  outline.hidden = !box;
  if (box) {
    Object.assign(outline.style, {
      left: `${box.l * k}px`, top: `${box.t * k}px`,
      width: `${(box.r - box.l) * k}px`, height: `${(box.b - box.t) * k}px`,
    });
  }
  caretEl.hidden = !(editing && wasm.caret());
  if (!caretEl.hidden) {
    const [x, y, h] = new Float32Array(wasm.memory.buffer, wasm.rect_ptr(), 3);
    Object.assign(caretEl.style, {
      left: `${x * k}px`, top: `${y * k}px`, height: `${h * k}px`, color: css(style.color),
    });
    Object.assign(typing.style, { left: `${x * k}px`, top: `${y * k}px` });
  }
}

const css = (color) => `#${color.toString(16).padStart(6, '0')}`;

// ---- Toolbar -----------------------------------------------------------------

function buildToolbar() {
  const tools = $('.tools');
  TOOLS.forEach((tool, i) => {
    const button = document.createElement('button');
    button.innerHTML = `<svg viewBox="0 0 24 24">${tool.icon}</svg>`;
    button.title = `${t(tool.name)} (${tool.key.toUpperCase()})`;
    button.addEventListener('click', () => setTool(i));
    tools.append(button);
  });
  const colors = $('.colors');
  for (const color of PALETTE) {
    const button = document.createElement('button');
    button.style.background = css(color);
    button.dataset.color = color;
    button.addEventListener('click', () => setColor(color));
    colors.append(button);
  }
  const custom = document.createElement('label');
  custom.title = t('customColor');
  custom.innerHTML = '<input type="color">';
  custom.firstChild.addEventListener('input', (e) => setColor(parseInt(e.target.value.slice(1), 16)));
  colors.append(custom);

  $('.fill').addEventListener('click', () => {
    style.fill = !style.fill;
    sync();
  });
  sizeInput.addEventListener('input', () => setSize(Number(sizeInput.value)));
  for (const button of document.querySelectorAll('[data-tip]')) button.title ||= t(button.dataset.tip);
  for (const button of document.querySelectorAll('[data-action]')) {
    button.addEventListener('click', () => act(button.dataset.action));
  }
  sync();
}

function sync() {
  $('.tools').querySelectorAll('button').forEach((b, i) => b.setAttribute('aria-pressed', i === style.tool));
  for (const b of $('.colors').querySelectorAll('button')) b.setAttribute('aria-pressed', Number(b.dataset.color) === style.color);
  $('.fill').setAttribute('aria-pressed', style.fill);
  const text = style.tool === TEXT;
  [sizeInput.min, sizeInput.max] = text ? TEXT_SIZES : STROKES;
  sizeInput.value = text ? style.text : style.stroke;
  sizeOutput.value = `${sizeInput.value} px`;
}

function setTool(i) {
  finishText();
  style.tool = i;
  sync();
}

function setColor(color) {
  style.color = color;
  sync();
}

function setSize(value) {
  const [lo, hi] = style.tool === TEXT ? TEXT_SIZES : STROKES;
  value = Math.max(lo, Math.min(hi, value));
  if (style.tool === TEXT) style.text = value;
  else style.stroke = value;
  sync();
}

function act(action) {
  switch (action) {
    case 'undo':
      finishText();
      if (wasm.undo()) changedAll();
      break;
    case 'redo':
      finishText();
      if (wasm.redo()) changedAll();
      break;
    case 'copy': copy(); break;
    case 'png': save('image/png', 'png'); break;
    case 'jpg': save('image/jpeg', 'jpg'); break;
  }
}

function changedAll() {
  selected = -1;
  invalidate(null);
}

// ---- Drawing -----------------------------------------------------------------

function toImage(e) {
  const r = view.getBoundingClientRect();
  return { x: ((e.clientX - r.left) * view.width) / r.width, y: ((e.clientY - r.top) * view.height) / r.height };
}

function bindInput() {
  view.addEventListener('pointerdown', down);
  view.addEventListener('pointermove', move);
  view.addEventListener('pointerup', up);
  view.addEventListener('pointercancel', up);
  typing.addEventListener('input', () => {
    const before = bounds(-1);
    const bytes = new TextEncoder().encode(typing.value);
    const ptr = wasm.alloc(bytes.length);
    new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes);
    wasm.set_text(ptr, bytes.length);
    invalidate(union(before, bounds(-1)));
  });
  typing.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      finishText();
    }
    e.stopPropagation();
  });
  addEventListener('keydown', key);
  addEventListener('resize', updateOverlays);
}

function down(e) {
  if (e.button !== 0) return;
  const p = toImage(e);
  if (editing) {
    finishText();
    return;
  }
  view.setPointerCapture(e.pointerId);
  const hit = wasm.hit(p.x, p.y);
  if (hit >= 0) {
    drag = { move: hit, last: p, moved: false };
    return;
  }
  if (selected >= 0) {
    selected = -1;
    updateOverlays();
  }
  const size = (style.tool === TEXT ? style.text : style.stroke) * scale;
  wasm.begin(style.tool, p.x, p.y, style.color, size, style.fill);
  hides ||= HIDES.has(style.tool);
  if (style.tool === TEXT) {
    startText();
  } else if (style.tool === NUMBER) {
    wasm.commit();
    invalidate(bounds(wasm.count() - 1));
  } else {
    drag = { draw: true };
    invalidate(bounds(-1));
  }
}

function move(e) {
  const p = toImage(e);
  if (!drag) {
    view.classList.toggle('over-shape', !editing && wasm.hit(p.x, p.y) >= 0);
    return;
  }
  if (drag.draw) {
    const before = bounds(-1);
    wasm.drag(p.x, p.y, e.shiftKey);
    invalidate(union(before, bounds(-1)));
    return;
  }
  if (!drag.moved) {
    drag.move = wasm.begin_move(drag.move);
    drag.moved = true;
  }
  const before = bounds(drag.move);
  wasm.translate(drag.move, p.x - drag.last.x, p.y - drag.last.y);
  drag.last = p;
  selected = drag.move;
  invalidate(union(before, bounds(drag.move)));
}

function up() {
  if (!drag) return;
  if (drag.draw) {
    const area = bounds(-1);
    wasm.commit();
    invalidate(area);
  } else if (!drag.moved) {
    selected = drag.move;
    updateOverlays();
  }
  drag = null;
}

function startText() {
  editing = true;
  typing.value = '';
  typing.hidden = false;
  updateOverlays();
  typing.focus();
}

function finishText() {
  if (!editing) return;
  editing = false;
  typing.hidden = true;
  const area = bounds(-1);
  wasm.commit();
  invalidate(area);
}

function key(e) {
  const command = e.ctrlKey || e.metaKey;
  const k = e.key.toLowerCase();
  if (command && k === 'z') act(e.shiftKey ? 'redo' : 'undo');
  else if (command && k === 'y') act('redo');
  else if (command && k === 'c') copy();
  else if (command && k === 's') act(e.shiftKey ? 'jpg' : 'png');
  else if (command || e.altKey) return;
  else if ((e.key === 'Delete' || e.key === 'Backspace') && selected >= 0) {
    wasm.remove(selected);
    changedAll();
  } else if (e.key === 'Escape' && selected >= 0) {
    selected = -1;
    updateOverlays();
  } else if (k === 'f') {
    style.fill = !style.fill;
    sync();
  } else if (e.key === '[' || e.key === ']') {
    setSize(Number(sizeInput.value) + (e.key === ']' ? 1 : -1) * (style.tool === TEXT ? 2 : 1));
  } else {
    const i = TOOLS.findIndex((tool) => tool.key === k);
    if (i < 0) return;
    setTool(i);
  }
  e.preventDefault();
}

// ---- Output ------------------------------------------------------------------

async function output(type) {
  finishText();
  if (frame) {
    cancelAnimationFrame(frame);
    flush();
  }
  if (type === 'image/png') return new Promise((resolve) => view.toBlob(resolve, type));
  // JPEG has no transparency: put the image on white.
  const canvas = new OffscreenCanvas(view.width, view.height);
  const c = canvas.getContext('2d');
  c.fillStyle = '#fff';
  c.fillRect(0, 0, view.width, view.height);
  c.drawImage(view, 0, 0);
  return canvas.convertToBlob({ type, quality: 0.92 });
}

async function copy() {
  try {
    const blob = await output('image/png');
    await navigator.clipboard.write([new ClipboardItem({ 'image/png': blob })]);
    toast(t('copied'));
  } catch (err) {
    toast(t('copyFailed', err.message || String(err)));
  }
}

async function save(type, ext) {
  const blob = await output(type);
  const url = URL.createObjectURL(blob);
  try {
    await chrome.downloads.download({ url, filename: `${fileName()}.${ext}`, saveAs: true });
  } catch (err) {
    // Cancelling the dialog lands here too.
    if (!/cancel/i.test(err.message)) toast(err.message);
  } finally {
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  }
}

// "Snapt_2026-03-07_09-05-02", as the desktop app's default.
function fileName() {
  const d = new Date();
  const p = (n) => String(n).padStart(2, '0');
  return `Snapt_${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}_${p(d.getHours())}-${p(d.getMinutes())}-${p(d.getSeconds())}`;
}

let toastTimer;

function toast(text) {
  const el = $('.toast');
  el.textContent = text;
  el.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (el.hidden = true), 2500);
}
