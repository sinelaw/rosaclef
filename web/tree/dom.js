// The browser backend of the tree UI library (./tree.js): the DOM backend, the
// event layer every listener goes through, pointer gestures and canvas sizing.
//
// This is the only part of the library that touches the browser, so it is
// plain (unchecked) JavaScript. Hosts expose it to checked code through their
// own declarations (Rosaclef re-exports it from "#platform": see
// web/types/platform.d.js).

// ------------------------------------------------------------------ events

// Every event a listener sees is "enriched": the facts a view needs (the
// target's box and scroll, a field's value, whether the user is typing) are
// read here, so view code never touches DOM nodes.
/** A terminal (xterm.js) takes the keys typed into it. */
const TERMINAL = ".xterm";
/** What counts as a control: a press on one does not take the keyboard back (Ev.onControl). */
const CONTROLS = "button, select, input, textarea, a, .knob, .fader, .lcd";

function isTyping(target) {
  if (!target || !target.closest) return false;
  const tag = (target.tagName || "").toLowerCase();
  return tag === "input" || tag === "textarea" || tag === "select" || target.isContentEditable || !!target.closest(TERMINAL);
}

function enrich(e) {
  if (e.__rc) return e;
  const t = e.currentTarget && e.currentTarget.getBoundingClientRect ? e.currentTarget : null;
  const r = t ? t.getBoundingClientRect() : null;
  const src = e.target || {};
  const extra = {
    typing: isTyping(e.target),
    terminal: !!(src.closest && src.closest(TERMINAL)),
    onControl: !!(src.closest && src.closest(CONTROLS)),
    value: src.value !== undefined ? String(src.value) : "",
    checked: !!src.checked,
    targetLeft: r ? r.left : 0,
    targetTop: r ? r.top : 0,
    targetWidth: r ? r.width : 0,
    targetHeight: r ? r.height : 0,
    scrollLeft: t ? t.scrollLeft : 0,
    scrollTop: t ? t.scrollTop : 0,
  };
  for (const k of Object.keys(extra)) {
    try {
      Object.defineProperty(e, k, { value: extra[k], configurable: true });
    } catch (_) {
      /* ignore */
    }
  }
  try {
    Object.defineProperty(e, "__rc", { value: true });
  } catch (_) {
    /* ignore */
  }
  return e;
}

function wrap(fn) {
  return (e) => fn(enrich(e));
}

export function listen(target, type, fn) {
  const opts = type === "wheel" || type.startsWith("touch") ? { passive: false } : undefined;
  target.addEventListener(type, wrap(fn), opts);
}

export function listenWindow(type, fn) {
  window.addEventListener(type, wrap(fn));
}

export function capturePointer(el, id) {
  try {
    el.setPointerCapture(id);
  } catch (_) {
    /* ignore */
  }
}

// ------------------------------------------------------------------ canvas

/** Device pixels a CSS pixel (2 on most phones and Retina screens; it follows the browser's zoom). */
export function pixelRatio() {
  return window.devicePixelRatio || 1;
}

/** Size a canvas for the device pixel ratio and return its 2D context. */
export function canvas2d(canvas, width, height) {
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.floor(width * dpr));
  const h = Math.max(1, Math.floor(height * dpr));
  if (canvas.width !== w) canvas.width = w;
  if (canvas.height !== h) canvas.height = h;
  canvas.style.width = width + "px";
  canvas.style.height = height + "px";
  const ctx = canvas.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  if (!ctx.fillGradient) {
    ctx.fillGradient = (g) => {
      ctx.fillStyle = g;
    };
    ctx.strokeGradient = (g) => {
      ctx.strokeStyle = g;
    };
  }
  if (!ctx.roundRect) {
    ctx.roundRect = (x, y, w2, h2) => ctx.rect(x, y, w2, h2);
  }
  return ctx;
}

// ------------------------------------------------------------------ gestures

/** Track a pointer gesture on the window until the button is released. */
export function drag(start, onMove, onUp) {
  // Follow only the pointer that started the drag: other fingers on a touch
  // screen are drags of their own.
  const mine = (e) => start.pointerId === undefined || e.pointerId === start.pointerId;
  const move = wrap((e) => {
    if (mine(e)) onMove(e);
  });
  const up = wrap((e) => {
    if (!mine(e)) return;
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", up);
    onUp(e);
  });
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", up);
}

// A finger on a scrollable editor scrolls it; only a tap (lifted without
// moving) acts, as a click there would. Mouse and pen act at once.
export function pressOrTap(e, act) {
  if (e.pointerType !== "touch") {
    act(e);
    return;
  }
  const id = e.pointerId;
  const x0 = e.clientX;
  const y0 = e.clientY;
  const done = (u) => {
    if (u.pointerId !== id) return;
    window.removeEventListener("pointerup", done);
    window.removeEventListener("pointercancel", done);
    if (u.type !== "pointerup" || Math.hypot(u.clientX - x0, u.clientY - y0) > 10) return;
    act(e);
    // The finger is already up: end any drag the action started, where it began.
    window.dispatchEvent(new PointerEvent("pointerup", { clientX: x0, clientY: y0, pointerId: id, pointerType: "touch" }));
  };
  window.addEventListener("pointerup", done);
  window.addEventListener("pointercancel", done);
}

// ------------------------------------------------------------------ backend

/** The DOM backend for ./tree.js: primitive operations on handles. */
export function domBackend(rootId) {
  const nodes = [document.getElementById(rootId)];
  const texts = [];
  const SVG = new Set([
    "svg",
    "path",
    "circle",
    "g",
    "line",
    "rect",
    "polyline",
    "defs",
    "linearGradient",
    "radialGradient",
    "stop",
    "text",
    "filter",
    "feTurbulence",
    "feDisplacementMap",
    "feGaussianBlur",
    "feComponentTransfer",
    "feFuncA",
    "feMorphology",
    "feComposite",
    "feColorMatrix",
    "feSpecularLighting",
    "feDistantLight",
    "feMerge",
    "feMergeNode",
    "feOffset",
  ]);
  return {
    root: () => 0,
    create: (type) => {
      const el = SVG.has(type) ? document.createElementNS("http://www.w3.org/2000/svg", type) : document.createElement(type);
      nodes.push(el);
      return nodes.length - 1;
    },
    setText: (h, s) => {
      let t = texts[h];
      if (!t) {
        t = document.createTextNode("");
        texts[h] = t;
        nodes[h].insertBefore(t, nodes[h].firstChild);
      }
      t.data = s;
    },
    setClass: (h, c) => {
      nodes[h].setAttribute("class", c);
    },
    setAttr: (h, k, v) => {
      nodes[h].setAttribute(k, v);
    },
    removeAttr: (h, k) => {
      nodes[h].removeAttribute(k);
    },
    setStyle: (h, k, v) => {
      nodes[h].style.setProperty(k, v);
    },
    setProp: (h, k, v) => {
      const el = nodes[h];
      if (k === "checked" || k === "disabled") el[k] = v === "true";
      else if (k === "focus" || k === "select") {
        // "select": focus a text field with its text selected, to type over.
        if (v === "true")
          setTimeout(() => {
            el.focus();
            if (k === "select" && el instanceof HTMLInputElement) el.select();
          }, 0);
      } else if (k === "reveal") {
        // "reveal": scroll the node into view, only as far as it takes (not at all when it shows).
        if (v === "true")
          requestAnimationFrame(() => {
            if (el.isConnected) el.scrollIntoView({ block: "nearest", inline: "nearest" });
          });
      } else if (k === "scrollLeft" || k === "scrollTop") {
        // Scrolling a node that is not in the document yet is ignored.
        if (el.isConnected) el[k] = Number(v);
        else
          requestAnimationFrame(() => {
            el[k] = Number(v);
          });
      } else if (el[k] !== v) el[k] = v;
    },
    append: (p, c) => {
      nodes[p].appendChild(nodes[c]);
    },
    insert: (p, c, before) => {
      nodes[p].insertBefore(nodes[c], nodes[before]);
    },
    remove: (h) => {
      const el = nodes[h];
      if (el) el.remove();
      nodes[h] = null;
      texts[h] = null;
    },
    listen: (h, event, fn) => {
      const el = nodes[h];
      if (event === "resize") {
        const ro = new ResizeObserver(() => fn(enrich({ currentTarget: el, target: el, preventDefault() {}, stopPropagation() {} })));
        ro.observe(el);
        return;
      }
      if (event === "mount") {
        setTimeout(() => fn(enrich({ currentTarget: el, target: el, preventDefault() {}, stopPropagation() {} })), 0);
        return;
      }
      const passive = event === "wheel" ? { passive: false } : undefined;
      el.addEventListener(event, (e) => fn(enrich(e)), passive);
    },
    paint: (h, fn) => {
      const el = nodes[h];
      const w = el.clientWidth;
      const hh = el.clientHeight;
      if (w === 0 || hh === 0) return;
      const ctx = canvas2d(el, w, hh);
      ctx.clearRect(0, 0, w, hh);
      fn(ctx, w, hh);
    },
    frame: (fn) => {
      requestAnimationFrame(() => fn());
    },
  };
}
