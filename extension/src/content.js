// The content script: Alt+click on an image opens it in a lightbox over the page,
// area selection and scrolling for screenshots, and the page side of saving
// images (blob URLs, error toasts).

(() => {
  // A copy left over from before the extension was updated or reloaded can no
  // longer reach the extension; it hands the page over to this one.
  document.dispatchEvent(new CustomEvent('snapt:replace'));
  const alive = new AbortController();
  document.addEventListener('snapt:replace', () => alive.abort(), { once: true });

  const t = (key) => chrome.i18n.getMessage(key);

  chrome.runtime.onMessage.addListener((message, _sender, reply) => {
    if (message.type === 'fetch') {
      fetchAsDataUrl(message.url).then(
        (dataUrl) => reply({ dataUrl }),
        (err) => reply({ error: err.message || String(err) }),
      );
      return true;
    }
    if (message.type === 'toast') toast(message.text);
    if (message.type === 'select') {
      selectArea(message.image).then(reply);
      return true;
    }
    if (message.type === 'page-start') reply(pageStart());
    if (message.type === 'page-scroll') {
      pageScroll(message.y, message.first).then(reply);
      return true;
    }
    if (message.type === 'page-end') reply(pageEnd());
    return false;
  });

  async function fetchAsDataUrl(url) {
    const response = await fetch(url);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const blob = await response.blob();
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(reader.result);
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(blob);
    });
  }

  // An isolated, unstyled host for our UI, so the page's CSS can't reach it.
  function createHost() {
    const host = document.createElement('div');
    host.style.cssText = 'all: initial !important; position: fixed !important; inset: 0 !important; '
      + 'z-index: 2147483647 !important; display: block !important;';
    const root = host.attachShadow({ mode: 'closed' });
    (document.body || document.documentElement).append(host);
    return { host, root };
  }

  function toast(text) {
    const { host, root } = createHost();
    host.style.setProperty('inset', 'auto 16px auto auto', 'important');
    host.style.setProperty('top', '16px', 'important');
    root.innerHTML = `<style>
      div { font: 500 13px/1.4 system-ui, sans-serif; color: #fff; background: #1f1b2e;
        border-left: 3px solid #8b5cf6; border-radius: 8px; padding: 10px 14px; max-width: 360px;
        box-shadow: 0 8px 24px rgb(0 0 0 / .35); transition: opacity .3s; }
    </style><div></div>`;
    root.querySelector('div').textContent = text;
    setTimeout(() => {
      root.querySelector('div').style.opacity = '0';
      setTimeout(() => host.remove(), 300);
    }, 4000);
  }

  // ---- Area selection ---------------------------------------------------------

  // Shows the frozen screenshot of the tab and lets the user drag out an area,
  // or click the element under the cursor. Resolves to the area in CSS pixels
  // of the viewport, or null when cancelled with Esc or a right click.
  function selectArea(image) {
    return new Promise((resolve) => {
      const { host, root } = createHost();
      root.innerHTML = SELECT_TEMPLATE;
      root.querySelector('.shot').src = image;
      const box = root.querySelector('.box');
      const label = root.querySelector('.label');
      const surface = root.querySelector('.surface');
      const hint = root.querySelector('.hint');
      hint.textContent = t('selectHint');
      const viewport = { width: innerWidth, height: innerHeight };
      let start = null;
      let rect = null;

      const show = (r) => {
        rect = r;
        box.hidden = !r;
        if (!r) return;
        Object.assign(box.style, { left: `${r.x}px`, top: `${r.y}px`, width: `${r.width}px`, height: `${r.height}px` });
        const dpr = devicePixelRatio || 1;
        label.textContent = `${Math.round(r.width * dpr)} × ${Math.round(r.height * dpr)}`;
        label.style.top = r.y > 28 ? '-26px' : '6px';
      };

      // The page element under the cursor, clipped to the window: one click
      // takes a card, an image or a code block without dragging.
      const elementAt = (x, y) => {
        host.style.setProperty('pointer-events', 'none', 'important');
        const el = document.elementFromPoint(x, y);
        host.style.removeProperty('pointer-events');
        if (!el || el === document.documentElement || el === document.body) return null;
        const r = el.getBoundingClientRect();
        const x0 = Math.max(0, r.left);
        const y0 = Math.max(0, r.top);
        const x1 = Math.min(viewport.width, r.right);
        const y1 = Math.min(viewport.height, r.bottom);
        return x1 - x0 > 4 && y1 - y0 > 4 ? { x: x0, y: y0, width: x1 - x0, height: y1 - y0 } : null;
      };

      const finish = (result) => {
        removeEventListener('keydown', onKey, true);
        host.remove();
        // Let the overlay disappear before anything else happens.
        requestAnimationFrame(() => resolve(result && { rect: result, viewport }));
      };
      const onKey = (e) => {
        if (e.key === 'Escape') finish(null);
        else if (e.key === 'Enter') finish(rect ?? { x: 0, y: 0, ...viewport });
        else return;
        e.preventDefault();
        e.stopImmediatePropagation();
      };
      addEventListener('keydown', onKey, true);

      surface.addEventListener('contextmenu', (e) => e.preventDefault());
      surface.addEventListener('pointerdown', (e) => {
        if (e.button !== 0) {
          finish(null);
          return;
        }
        surface.setPointerCapture(e.pointerId);
        start = { x: e.clientX, y: e.clientY, dragged: false };
        hint.hidden = true;
      });
      surface.addEventListener('pointermove', (e) => {
        if (!start) {
          show(elementAt(e.clientX, e.clientY));
          return;
        }
        if (!start.dragged && Math.hypot(e.clientX - start.x, e.clientY - start.y) < 4) return;
        start.dragged = true;
        const x = Math.max(0, Math.min(e.clientX, viewport.width));
        const y = Math.max(0, Math.min(e.clientY, viewport.height));
        show({
          x: Math.min(start.x, x),
          y: Math.min(start.y, y),
          width: Math.abs(x - start.x),
          height: Math.abs(y - start.y),
        });
      });
      surface.addEventListener('pointerup', () => {
        if (!start) return;
        const picked = start.dragged ? rect : rect ?? { x: 0, y: 0, ...viewport };
        start = null;
        if (picked && picked.width >= 2 && picked.height >= 2) finish(picked);
      });
    });
  }

  const SELECT_TEMPLATE = `<style>
    .shot { position: fixed; inset: 0; width: 100vw; height: 100vh; user-select: none; }
    .surface { position: fixed; inset: 0; cursor: crosshair; background: rgb(0 0 0 / .4); }
    .box { position: fixed; outline: 1px dashed #fff; box-shadow: 0 0 0 1px #8b5cf6;
      background: transparent; backdrop-filter: brightness(1.67); pointer-events: none; }
    .label { position: absolute; left: 0; padding: 3px 6px; border-radius: 5px; background: #8b5cf6; color: #fff;
      font: 600 11px/1.2 system-ui, sans-serif; white-space: nowrap; font-variant-numeric: tabular-nums; }
    .hint { position: fixed; left: 50%; top: 18px; transform: translateX(-50%); padding: 8px 14px;
      border-radius: 10px; background: rgb(28 25 38 / .92); color: #e8e6f0; font: 500 13px/1.3 system-ui, sans-serif;
      pointer-events: none; }
  </style>
  <img class="shot" alt="">
  <div class="surface"></div>
  <div class="box" hidden><span class="label"></span></div>
  <div class="hint"></div>`;

  // ---- Whole page -------------------------------------------------------------

  let page = null;

  // Prepares the page for capture frame by frame: no scrollbar, no smooth
  // scrolling, and a list of the fixed and sticky elements that would repeat on
  // every frame.
  function pageStart() {
    const style = document.createElement('style');
    style.textContent = `html { scrollbar-width: none !important; scroll-behavior: auto !important; }
      html::-webkit-scrollbar { display: none !important; }`;
    document.documentElement.append(style);
    const pinned = [];
    for (const el of document.querySelectorAll('body *')) {
      const position = getComputedStyle(el).position;
      if (position === 'fixed' || position === 'sticky') pinned.push([el, el.style.getPropertyValue('visibility'), el.style.getPropertyPriority('visibility')]);
    }
    page = { style, pinned, x: scrollX, y: scrollY };
    const scroller = document.scrollingElement || document.documentElement;
    return {
      width: innerWidth,
      height: innerHeight,
      total: Math.max(scroller.scrollHeight, innerHeight),
      dpr: devicePixelRatio || 1,
    };
  }

  // Scrolls to `y` and returns where the page actually ended up.
  async function pageScroll(y, first) {
    if (!page) return y;
    // Headers and floating buttons stay on the first frame only.
    if (!first) for (const [el] of page.pinned) el.style.setProperty('visibility', 'hidden', 'important');
    scrollTo(0, y);
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    return scrollY;
  }

  function pageEnd() {
    if (!page) return true;
    for (const [el, value, priority] of page.pinned) el.style.setProperty('visibility', value, priority);
    page.style.remove();
    scrollTo(page.x, page.y);
    page = null;
    return true;
  }

  // ---- Lightbox -------------------------------------------------------------

  addEventListener('click', (e) => {
    if (!e.altKey || e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey) return;
    if (lightbox) return;
    const found = findImage(e);
    if (!found) return;
    // Alt+click on a link downloads it in Chrome; the lightbox replaces that.
    e.preventDefault();
    e.stopImmediatePropagation();
    open(found);
  }, { capture: true, signal: alive.signal });

  // The image under the cursor: an <img> in the click path, one covered by
  // another element (galleries often put a link or overlay on top), or a CSS
  // background image.
  function findImage(e) {
    const loaded = (el) => el instanceof HTMLImageElement && el.complete && (el.naturalWidth > 0 || isSvg(el));
    const path = e.composedPath().filter((el) => el instanceof Element);
    const img = path.find(loaded)
      || document.elementsFromPoint(e.clientX, e.clientY).find(loaded);
    if (img) return { source: img, rect: img.getBoundingClientRect() };
    for (const el of path.slice(0, 6)) {
      const match = /url\(["']?(.+?)["']?\)/.exec(getComputedStyle(el).backgroundImage);
      if (match) return { url: new URL(match[1], document.baseURI).href, rect: el.getBoundingClientRect() };
    }
    return null;
  }

  const isSvg = (img) => /\.svg(\?|#|$)|^data:image\/svg/i.test(img.currentSrc || img.src);

  const MAX_SCALE = 64;
  // Device pixels per image pixel from which the pixel grid shows, fully at twice that.
  const GRID_FROM = 8;
  // How quickly the view catches up with its target, in milliseconds.
  const EASE = 70;
  const MARGIN = 24;

  let lightbox = null;

  async function open({ source, url, rect }) {
    if (!source) {
      source = new Image();
      source.src = url;
      try {
        await source.decode();
      } catch {
        return;
      }
    }
    lightbox = new Lightbox(source, rect);
  }

  class Lightbox {
    constructor(img, rect) {
      this.img = img;
      this.origin = img.isConnected ? img : null;
      this.w = img.naturalWidth || rect.width;
      this.h = img.naturalHeight || rect.height;
      ({ host: this.host, root: this.root } = createHost());
      this.root.innerHTML = TEMPLATE;
      this.canvas = this.root.querySelector('canvas');
      this.ctx = this.canvas.getContext('2d');
      this.backdrop = this.root.querySelector('.backdrop');
      this.label = this.root.querySelector('.zoom');
      this.size = this.root.querySelector('.size');
      this.size.textContent = `${Math.round(this.w)} × ${Math.round(this.h)}`;
      for (const button of this.root.querySelectorAll('button')) {
        button.title = t(button.dataset.tip);
        button.addEventListener('click', () => this.action(button.dataset.action));
      }

      this.resize();
      // Starts over the image on the page and grows to fit the window.
      this.cur = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2, s: rect.width / this.w, a: 0 };
      this.fitted = true;
      this.tgt = this.fit();
      this.closing = false;
      this.drag = null;

      this.listeners = [
        [window, 'keydown', (e) => this.key(e), true],
        [window, 'resize', () => this.resize(), false],
        [this.host, 'wheel', (e) => this.wheel(e), false],
        [this.canvas, 'pointerdown', (e) => this.down(e), false],
        [this.canvas, 'pointermove', (e) => this.move(e), false],
        [this.canvas, 'pointerup', (e) => this.up(e), false],
        [this.canvas, 'dblclick', (e) => this.toggleZoom(e.clientX, e.clientY), false],
      ];
      for (const [target, type, fn, capture] of this.listeners) {
        target.addEventListener(type, fn, { capture, passive: false });
      }
      requestAnimationFrame(() => this.backdrop.classList.add('shown'));
      this.last = performance.now();
      this.frame = requestAnimationFrame((now) => this.tick(now));
    }

    // The view that fits the (rotated) image in the window, never enlarging it.
    fit() {
      const quarter = Math.round(this.tgt?.a / 90 || 0);
      const turned = Math.abs(quarter) % 2 === 1;
      const w = turned ? this.h : this.w;
      const h = turned ? this.w : this.h;
      const s = Math.min((this.vw - 2 * MARGIN) / w, (this.vh - 2 * MARGIN - 56) / h, 1);
      return { x: this.vw / 2, y: (this.vh - 56) / 2, s, a: quarter * 90 };
    }

    minScale() {
      return Math.min(this.fit().s, 1) / 4;
    }

    resize() {
      this.vw = innerWidth;
      this.vh = innerHeight;
      this.dpr = devicePixelRatio || 1;
      this.canvas.width = Math.round(this.vw * this.dpr);
      this.canvas.height = Math.round(this.vh * this.dpr);
      if (this.fitted && this.tgt) this.tgt = this.fit();
      this.dirty = true;
    }

    // Zooms to `s`, keeping the image point under (`px`, `py`) where it is.
    // Scale and center move linearly together, which keeps that point fixed
    // during the animation too.
    zoomTo(s, px, py) {
      s = Math.min(Math.max(s, this.minScale()), MAX_SCALE);
      const k = s / this.tgt.s;
      this.tgt = { ...this.tgt, s, x: px - (px - this.tgt.x) * k, y: py - (py - this.tgt.y) * k };
      this.fitted = false;
      this.wake();
    }

    toggleZoom(px, py) {
      if (this.fitted || Math.abs(this.tgt.s - this.fit().s) < 1e-3) {
        this.zoomTo(this.fit().s < 1 ? 1 : Math.min(this.tgt.s * 4, MAX_SCALE), px, py);
      } else {
        this.showFit();
      }
    }

    showFit() {
      this.tgt = this.fit();
      this.fitted = true;
      this.wake();
    }

    rotate(dir) {
      this.tgt = { ...this.tgt, a: this.tgt.a + dir * 90 };
      if (this.fitted) this.tgt = this.fit();
      this.wake();
    }

    action(name) {
      const cx = this.vw / 2;
      const cy = this.vh / 2;
      switch (name) {
        case 'in': this.zoomTo(this.tgt.s * 1.5, cx, cy); break;
        case 'out': this.zoomTo(this.tgt.s / 1.5, cx, cy); break;
        case 'fit': this.showFit(); break;
        case 'actual': this.zoomTo(1, cx, cy); break;
        case 'left': this.rotate(-1); break;
        case 'right': this.rotate(1); break;
        case 'close': this.close(); break;
      }
    }

    key(e) {
      if (this.closing) return;
      const map = {
        Escape: 'close', '+': 'in', '=': 'in', '-': 'out', _: 'out', 0: 'fit', 1: 'actual',
        r: e.shiftKey ? 'left' : 'right', R: e.shiftKey ? 'left' : 'right',
      };
      const pan = { ArrowLeft: [80, 0], ArrowRight: [-80, 0], ArrowUp: [0, 80], ArrowDown: [0, -80] }[e.key];
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (map[e.key]) {
        this.action(map[e.key]);
      } else if (pan) {
        this.tgt = { ...this.tgt, x: this.tgt.x + pan[0], y: this.tgt.y + pan[1] };
        this.fitted = false;
        this.wake();
      } else if (![' ', 'PageUp', 'PageDown', 'Home', 'End'].includes(e.key)) {
        return;
      }
      // The lightbox is modal: keys it handles, and page scrolling, stay with it.
      e.preventDefault();
      e.stopImmediatePropagation();
    }

    wheel(e) {
      e.preventDefault();
      e.stopPropagation();
      if (this.closing) return;
      const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? this.vh : 1;
      // Pinch on a touchpad arrives as a wheel with Ctrl and small deltas.
      const speed = e.ctrlKey ? 0.01 : 0.0015;
      const delta = Math.max(-300, Math.min(300, e.deltaY * unit));
      this.zoomTo(this.tgt.s * Math.exp(-delta * speed), e.clientX, e.clientY);
    }

    down(e) {
      if (e.button !== 0 || this.closing) return;
      this.canvas.setPointerCapture(e.pointerId);
      this.drag = { x: e.clientX, y: e.clientY, moved: false };
    }

    move(e) {
      if (!this.drag) return;
      const dx = e.clientX - this.drag.x;
      const dy = e.clientY - this.drag.y;
      if (!this.drag.moved && Math.hypot(dx, dy) < 4) return;
      this.drag.moved = true;
      this.canvas.classList.add('dragging');
      this.drag.x = e.clientX;
      this.drag.y = e.clientY;
      // Panning follows the pointer without easing.
      this.tgt = { ...this.tgt, x: this.tgt.x + dx, y: this.tgt.y + dy };
      this.cur = { ...this.cur, x: this.cur.x + dx, y: this.cur.y + dy };
      this.fitted = false;
      this.wake();
    }

    up(e) {
      if (!this.drag) return;
      const moved = this.drag.moved;
      this.drag = null;
      this.canvas.classList.remove('dragging');
      // A click on the backdrop, outside the image, closes the lightbox.
      if (!moved && !this.hit(e.clientX, e.clientY)) this.close();
    }

    hit(px, py) {
      const { x, y, s, a } = this.cur;
      const r = (-a * Math.PI) / 180;
      const dx = px - x;
      const dy = py - y;
      const ix = (dx * Math.cos(r) - dy * Math.sin(r)) / s;
      const iy = (dx * Math.sin(r) + dy * Math.cos(r)) / s;
      return Math.abs(ix) <= this.w / 2 && Math.abs(iy) <= this.h / 2;
    }

    close() {
      if (this.closing) return;
      this.closing = true;
      this.backdrop.classList.remove('shown');
      this.root.querySelector('.bar').classList.add('hidden');
      const a = Math.round(this.tgt.a / 360) * 360;
      const rect = this.origin?.getBoundingClientRect();
      const visible = rect && rect.width > 0 && rect.bottom > 0 && rect.top < this.vh
        && rect.right > 0 && rect.left < this.vw;
      if (visible) {
        this.tgt = { x: rect.left + rect.width / 2, y: rect.top + rect.height / 2, s: rect.width / this.w, a };
      } else {
        this.canvas.classList.add('fading');
      }
      this.wake();
      setTimeout(() => this.destroy(), 320);
    }

    destroy() {
      cancelAnimationFrame(this.frame);
      for (const [target, type, fn, capture] of this.listeners) {
        target.removeEventListener(type, fn, { capture });
      }
      this.host.remove();
      lightbox = null;
    }

    wake() {
      this.dirty = true;
      if (!this.frame) {
        this.last = performance.now();
        this.frame = requestAnimationFrame((now) => this.tick(now));
      }
    }

    tick(now) {
      this.frame = 0;
      const k = 1 - Math.exp(-(now - this.last) / EASE);
      this.last = now;
      let moving = false;
      for (const key of ['x', 'y', 's', 'a']) {
        const d = this.tgt[key] - this.cur[key];
        const done = key === 's' ? Math.abs(d) < this.tgt.s * 1e-4 : Math.abs(d) < 0.05;
        this.cur[key] = done ? this.tgt[key] : this.cur[key] + d * k;
        moving ||= !done;
      }
      if (moving || this.dirty) this.draw();
      this.dirty = false;
      if (moving) this.frame = requestAnimationFrame((n) => this.tick(n));
    }

    draw() {
      const { ctx, w, h, dpr } = this;
      const { x, y, s, a } = this.cur;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.translate(x, y);
      ctx.rotate((a * Math.PI) / 180);
      ctx.scale(s, s);
      ctx.translate(-w / 2, -h / 2);

      // A checkerboard under the image, so transparent images stay readable.
      ctx.save();
      ctx.beginPath();
      ctx.rect(0, 0, w, h);
      ctx.clip();
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.fillStyle = checker(ctx, dpr);
      ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
      ctx.restore();

      const pixels = s * dpr;
      // Zoomed in, every image pixel is a sharp square.
      ctx.imageSmoothingEnabled = pixels < 2;
      ctx.imageSmoothingQuality = 'high';
      ctx.drawImage(this.img, 0, 0, w, h);

      if (pixels >= GRID_FROM) this.grid(Math.min(1, (pixels - GRID_FROM) / GRID_FROM));
      this.label.textContent = `${Math.round(s * 100)}%`;
    }

    // Lines between image pixels, only across the part of the image in view.
    grid(alpha) {
      const { ctx, w, h } = this;
      const inv = ctx.getTransform().inverse();
      const corners = [[0, 0], [this.canvas.width, 0], [0, this.canvas.height], [this.canvas.width, this.canvas.height]]
        .map(([cx, cy]) => inv.transformPoint(new DOMPoint(cx, cy)));
      const x0 = Math.max(0, Math.ceil(Math.min(...corners.map((p) => p.x))));
      const x1 = Math.min(w, Math.floor(Math.max(...corners.map((p) => p.x))));
      const y0 = Math.max(0, Math.ceil(Math.min(...corners.map((p) => p.y))));
      const y1 = Math.min(h, Math.floor(Math.max(...corners.map((p) => p.y))));
      ctx.beginPath();
      for (let gx = x0; gx <= x1; gx++) {
        ctx.moveTo(gx, Math.max(0, y0 - 1));
        ctx.lineTo(gx, Math.min(h, y1 + 1));
      }
      for (let gy = y0; gy <= y1; gy++) {
        ctx.moveTo(Math.max(0, x0 - 1), gy);
        ctx.lineTo(Math.min(w, x1 + 1), gy);
      }
      ctx.lineWidth = 1 / (this.cur.s * this.dpr);
      ctx.strokeStyle = `rgb(128 128 128 / ${0.55 * alpha})`;
      ctx.stroke();
    }
  }

  let checkerPattern = null;

  function checker(ctx, dpr) {
    if (checkerPattern) return checkerPattern;
    const size = Math.round(8 * dpr);
    const tile = new OffscreenCanvas(size * 2, size * 2);
    const c = tile.getContext('2d');
    c.fillStyle = '#3a3a40';
    c.fillRect(0, 0, size * 2, size * 2);
    c.fillStyle = '#4a4a52';
    c.fillRect(0, 0, size, size);
    c.fillRect(size, size, size, size);
    checkerPattern = ctx.createPattern(tile, 'repeat');
    return checkerPattern;
  }

  const icon = (d) => `<svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor"
    stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${d}</svg>`;

  const TEMPLATE = `<style>
    :host { color-scheme: dark; }
    .backdrop { position: fixed; inset: 0; background: rgb(12 10 18 / .94); opacity: 0;
      transition: opacity .25s ease; }
    .backdrop.shown { opacity: 1; }
    canvas { position: fixed; inset: 0; width: 100%; height: 100%; cursor: grab; transition: opacity .25s; }
    canvas.dragging { cursor: grabbing; }
    canvas.fading { opacity: 0; }
    .bar { position: fixed; left: 50%; bottom: 14px; transform: translateX(-50%); display: flex;
      align-items: center; gap: 2px; padding: 4px; border-radius: 12px; background: rgb(28 25 38 / .92);
      box-shadow: 0 8px 28px rgb(0 0 0 / .45); font: 500 12px/1 system-ui, sans-serif; color: #e8e6f0;
      user-select: none; transition: opacity .2s; }
    .bar.hidden { opacity: 0; pointer-events: none; }
    button { all: unset; display: grid; place-items: center; width: 32px; height: 32px; border-radius: 8px;
      color: inherit; cursor: pointer; }
    button:hover { background: rgb(255 255 255 / .1); }
    button:focus-visible { outline: 2px solid #8b5cf6; }
    .zoom { min-width: 48px; text-align: center; font-variant-numeric: tabular-nums; }
    .size { padding: 0 10px; color: #a29cb8; font-variant-numeric: tabular-nums; }
    .sep { width: 1px; height: 20px; margin: 0 4px; background: rgb(255 255 255 / .14); }
  </style>
  <div class="backdrop"></div>
  <canvas></canvas>
  <div class="bar">
    <span class="size"></span>
    <span class="sep"></span>
    <button data-action="out" data-tip="zoomOut">${icon('<path d="M5 12h14"/>')}</button>
    <span class="zoom"></span>
    <button data-action="in" data-tip="zoomIn">${icon('<path d="M12 5v14M5 12h14"/>')}</button>
    <span class="sep"></span>
    <button data-action="fit" data-tip="fit">${icon('<path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"/>')}</button>
    <button data-action="actual" data-tip="actual">${icon('<path d="M7 8l2-1v10M15 8l2-1v10M12.5 10v.01M12.5 15v.01"/>')}</button>
    <button data-action="left" data-tip="rotateLeft">${icon('<path d="M3 4v5h5"/><path d="M3.5 9a9 9 0 1 1-.5 4"/>')}</button>
    <button data-action="right" data-tip="rotateRight">${icon('<path d="M21 4v5h-5"/><path d="M20.5 9a9 9 0 1 0 .5 4"/>')}</button>
    <span class="sep"></span>
    <button data-action="close" data-tip="close">${icon('<path d="M6 6l12 12M18 6L6 18"/>')}</button>
  </div>`;
})();
