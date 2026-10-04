// The film's renderer, the three.js way: the same frames as web/lib/filmgl.js
// (a GlFrame), drawn by three.js — its physically based materials, its
// image-based light (the HDR room prefiltered by PMREM, lighting paper and ink
// alike), the lamp, tone mapping and color management. What is ours: the
// desk's layout, the pages' bitmaps, and the drops' heights (web/lib/inkdrops.js)
// given to three.js as a normal map and a roughness map (wet ink glossy, the
// paper matte). Chosen with ?renderer=three (web/lib/platform.js), to compare.
//
// The world is y up: a desk point (x, y) is (x, 0, y), the camera above it.
// Not type-checked (three.js is outside inty's library); web/types/platform.d.js
// types what platform.js exports.

import { inkHeights } from "./inkdrops.js";
import { encodeWith } from "./filmgl.js";

/** The lens's vertical field of view (degrees), as web/lib/filmgl.js has it. */
const FOV = 32;
/** The room's turn about the vertical (radians): its windows to the left of the page, as in web/lib/filmgl.js. */
const ENV_ROT = 0.83;
/** Wet ink, and the paper. */
const INK_ROUGH = 0.07;
const PAPER_ROUGH = 0.9;

let THREE = null;
let HDRLoader = null;
let loading = null;

/** three.js, loaded the first time a film is drawn this way (its module, once loaded). */
export function three() {
  return THREE;
}

/** three.js, loaded the first time a film is drawn this way. */
export function load() {
  if (!loading)
    loading = Promise.all([import("../vendor/three/three.module.js"), import("../vendor/three/HDRLoader.js")]).then(([t, h]) => {
      THREE = t;
      HDRLoader = h.HDRLoader;
    });
  return loading;
}

/** A soft dark rectangle (a page's shadow) and a vignette, as alpha maps. */
function softTexture(kind) {
  const c = document.createElement("canvas");
  c.width = 256;
  c.height = 256;
  const g = c.getContext("2d");
  const img = g.createImageData(256, 256);
  for (let y = 0; y < 256; y++)
    for (let x = 0; x < 256; x++) {
      const u = (x + 0.5) / 256;
      const v = (y + 0.5) / 256;
      let a;
      if (kind === "shadow") {
        const d = Math.min(u, 1 - u, v, 1 - v) / 0.06;
        a = Math.min(1, d) * Math.min(1, d) * (3 - 2 * Math.min(1, d));
      } else {
        const r = Math.hypot(u - 0.5, v - 0.5) / 0.7071;
        const t = Math.min(1, Math.max(0, (r - 0.35) / 0.85));
        a = t * t * (3 - 2 * t);
      }
      const k = (y * 256 + x) * 4;
      img.data[k] = img.data[k + 1] = img.data[k + 2] = Math.round(255 * a);
      img.data[k + 3] = 255;
    }
  g.putImageData(img, 0, 0);
  const t = new THREE.CanvasTexture(c);
  return t;
}

/** How much to scale the room (an equirectangular HDR image, rows from the top) so a white page under it alone shows as bright as 1. */
function roomGain(img) {
  const { width: w, height: h, data } = img;
  const half = data instanceof Uint16Array;
  const ch = data.length / (w * h);
  const val = (k) => (half ? THREE.DataUtils.fromHalfFloat(data[k]) : data[k]);
  let sum = 0;
  for (let y = 0; y < h; y += 2) {
    const el = (0.5 - (y + 0.5) / h) * Math.PI;
    const up = Math.sin(el);
    if (up <= 0) continue;
    const dA = ((2 * Math.PI) / w) * (Math.PI / h) * Math.cos(el) * 4;
    for (let x = 0; x < w; x += 2) {
      const k = (y * w + x) * ch;
      sum += (0.2126 * val(k) + 0.7152 * val(k + 1) + 0.0722 * val(k + 2)) * up * dA;
    }
  }
  return sum > 0 ? Math.PI / sum : 1;
}

/** A renderer on a canvas: { ready(frame), draw(frame), forget(url), canvas, onLoad }, and the scene it drew (for web/lib/filmpath.js). */
export function renderer(canvas) {
  const gl = new THREE.WebGLRenderer({ canvas, antialias: true, preserveDrawingBuffer: true });
  gl.outputColorSpace = THREE.SRGBColorSpace;
  gl.toneMapping = THREE.NeutralToneMapping;
  gl.toneMappingExposure = 1.6;
  gl.autoClear = false;
  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0x080605);
  const camera = new THREE.PerspectiveCamera(FOV, 1, 1, 1000);
  const lamp = new THREE.PointLight(0xfff3e0, 1, 0, 0);
  scene.add(lamp);
  // The room: the HDR image, prefiltered for every roughness (PMREM).
  let envReady = null;
  // The room's image as it is (equirectangular): the path tracer samples it (web/lib/filmpath.js).
  let roomImage = null;
  const room = new Promise((done) => {
    new HDRLoader().load(new URL("../vendor/hdri/artist_workshop_1k.hdr", import.meta.url).href, (tex) => {
      tex.mapping = THREE.EquirectangularReflectionMapping;
      const pm = new THREE.PMREMGenerator(gl);
      scene.environment = pm.fromEquirectangular(tex).texture;
      scene.environmentRotation.set(0, ENV_ROT, 0);
      // The room's light in the paper's units (as web/lib/filmgl.js has it): a
      // white page lit by the room alone shows as bright as 1.
      scene.environmentIntensity = roomGain(tex.image);
      roomImage = tex;
      pm.dispose();
      envReady = true;
      if (api.onLoad) api.onLoad();
      done(true);
    });
  });
  // The vignette, over the picture.
  const over = new THREE.Scene();
  const overCam = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
  const vignette = new THREE.Mesh(
    new THREE.PlaneGeometry(2, 2),
    new THREE.MeshBasicMaterial({ color: 0x000000, transparent: true, alphaMap: softTexture("vignette"), depthTest: false, depthWrite: false })
  );
  over.add(vignette);
  const shadowAlpha = softTexture("shadow");

  // Textures by URL: the color bitmaps (images), the inks' normal and roughness maps (made from their heights).
  const textures = new Map();
  const loader = new THREE.TextureLoader();
  const maxAniso = gl.capabilities.getMaxAnisotropy();
  function color(url, repeat) {
    if (!url) return null;
    let t = textures.get(url);
    if (!t) {
      t = { tex: null, promise: null };
      textures.set(url, t);
      t.promise = new Promise((done) =>
        loader.load(
          url,
          (tex) => {
            tex.colorSpace = THREE.SRGBColorSpace;
            tex.flipY = false;
            tex.anisotropy = Math.min(8, maxAniso);
            if (repeat) tex.wrapS = tex.wrapT = THREE.RepeatWrapping;
            tex.needsUpdate = true;
            t.tex = tex;
            if (api.onLoad) api.onLoad();
            done(true);
          },
          undefined,
          () => done(false)
        )
      );
    }
    return t.tex;
  }
  /** The ink's surface from its heights: a tangent-space normal map (u right, v down the page) and roughness (green). */
  function surface(url, scale) {
    if (!url) return null;
    let t = textures.get(url);
    if (!t) {
      const hm = inkHeights(url);
      t = { tex: null, rough: null, promise: null };
      textures.set(url, t);
      if (!hm) return null;
      const { w, h, data } = hm;
      const n = new Uint8Array(w * h * 4);
      const r = new Uint8Array(w * h * 4);
      const at = (x, y) => 1 - data[Math.min(h - 1, Math.max(0, y)) * w + Math.min(w - 1, Math.max(0, x))] / 65535;
      for (let y = 0; y < h; y++)
        for (let x = 0; x < w; x++) {
          // Slopes in points of height a point (the bitmap has `scale` pixels a point).
          const gx = ((at(x + 1, y) - at(x - 1, y)) / 2) * scale;
          const gy = ((at(x, y + 1) - at(x, y - 1)) / 2) * scale;
          const len = Math.hypot(gx, gy, 1);
          const k = (y * w + x) * 4;
          n[k] = Math.round((0.5 - (0.5 * gx) / len) * 255);
          n[k + 1] = Math.round((0.5 - (0.5 * gy) / len) * 255);
          n[k + 2] = Math.round((0.5 + 0.5 / len) * 255);
          n[k + 3] = 255;
          const hgt = at(x, y);
          const wet = Math.min(1, Math.max(0, (hgt - 0.002) / 0.018));
          r[k + 1] = Math.round((PAPER_ROUGH + (INK_ROUGH - PAPER_ROUGH) * wet) * 255);
          r[k + 3] = 255;
        }
      for (const [arr, key] of [
        [n, "tex"],
        [r, "rough"],
      ]) {
        const tex = new THREE.DataTexture(arr, w, h, THREE.RGBAFormat);
        tex.flipY = false;
        tex.generateMipmaps = true;
        tex.minFilter = THREE.LinearMipmapLinearFilter;
        tex.magFilter = THREE.LinearFilter;
        tex.anisotropy = Math.min(8, maxAniso);
        tex.needsUpdate = true;
        t[key] = tex;
      }
    }
    return t;
  }

  // Meshes, kept from frame to frame by what they show.
  const meshes = new Map();
  function quad(key, make) {
    let m = meshes.get(key);
    if (!m) {
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.BufferAttribute(new Float32Array(12), 3));
      geo.setAttribute("uv", new THREE.BufferAttribute(new Float32Array(8), 2));
      geo.setAttribute("normal", new THREE.BufferAttribute(new Float32Array([0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0]), 3));
      // Corners in the desk's order (top left, top right, bottom right, bottom left), wound to face up.
      geo.setIndex([0, 2, 1, 0, 3, 2]);
      m = new THREE.Mesh(geo, make());
      m.frustumCulled = false;
      scene.add(m);
      meshes.set(key, m);
    }
    m.visible = true;
    return m;
  }
  /** Place a quad: four desk points (x, y pairs) at a height, with its uvs. */
  function place(m, pts, uvs, lift) {
    const p = m.geometry.attributes.position;
    for (let i = 0; i < 4; i++) p.setXYZ(i, pts[i * 2], lift, pts[i * 2 + 1]);
    p.needsUpdate = true;
    const u = m.geometry.attributes.uv;
    for (let i = 0; i < 4; i++) u.setXY(i, uvs[i * 2], uvs[i * 2 + 1]);
    u.needsUpdate = true;
  }

  function draw(f) {
    const w = Math.max(1, Math.round(f.width));
    const h = Math.max(1, Math.round(f.height));
    gl.setPixelRatio(1);
    gl.setSize(w, h, false);
    const aspect = w / h;
    // The camera, as web/lib/filmgl.js places it (its desk z into the desk is our -y).
    const [x, y, span, tilt, turn] = f.cam;
    const t = (tilt * Math.PI) / 180;
    const r = (turn * Math.PI) / 180;
    const d = span / 2 / Math.tan((FOV * Math.PI) / 360) / aspect;
    const dx = -Math.sin(r);
    const dy = Math.cos(r);
    camera.aspect = aspect;
    camera.near = d * 0.02;
    camera.far = d * 20;
    camera.position.set(x + dx * d * Math.sin(t), d * Math.cos(t), y + dy * d * Math.sin(t));
    camera.up.set(-dx, 0, -dy);
    camera.lookAt(x, 0, y);
    camera.updateProjectionMatrix();
    lamp.position.set(f.light[0], f.light[2], f.light[1]);
    lamp.intensity = 0.3 * Math.PI;

    for (const m of meshes.values()) m.visible = false;
    let complete = true;
    let order = 0;
    // The desk.
    const dk = f.desk;
    const deskTex = color(f.deskTex, true);
    if (f.deskTex && !deskTex) complete = false;
    const desk = quad(
      "desk",
      () => new THREE.MeshStandardMaterial({ roughness: 0.6, metalness: 0, depthTest: false, depthWrite: false, side: THREE.FrontSide })
    );
    desk.material.map = deskTex;
    desk.material.color.set(deskTex ? 0xffffff : f.deskColor);
    desk.material.needsUpdate = true;
    const tile = f.deskTile;
    place(
      desk,
      [dk[0], dk[1], dk[2], dk[1], dk[2], dk[3], dk[0], dk[3]],
      [dk[0] / tile, dk[1] / tile, dk[2] / tile, dk[1] / tile, dk[2] / tile, dk[3] / tile, dk[0] / tile, dk[3] / tile],
      0
    );
    desk.renderOrder = order++;
    // The pages' shadows, away from the lamp.
    f.sheets.forEach((s, i) => {
      if (!s.page) return;
      const q = s.quad;
      const cx = (q[0] + q[4]) / 2;
      const cy = (q[1] + q[5]) / 2;
      const ox = (cx - f.light[0]) * 0.012;
      const oy = (cy - f.light[1]) * 0.012 + 6;
      const pts = [];
      for (let k = 0; k < 4; k++) pts.push(cx + (q[k * 2] - cx) * 1.06 + ox, cy + (q[k * 2 + 1] - cy) * 1.06 + oy);
      const sh = quad(
        `shadow${i}`,
        () =>
          new THREE.MeshBasicMaterial({
            color: 0x000000,
            transparent: true,
            opacity: 0.5,
            alphaMap: shadowAlpha,
            depthTest: false,
            depthWrite: false,
            side: THREE.FrontSide,
          })
      );
      place(sh, pts, [0, 0, 1, 0, 1, 1, 0, 1], 0);
      sh.renderOrder = order++;
    });
    // The pages, then the sharper tiles over them (in the frame's order: the finest last).
    for (const s of f.sheets) {
      const ct = color(s.color, false);
      const ink = surface(s.height, s.scale);
      if ((s.color && !ct) || (s.height && !(ink && ink.tex))) complete = false;
      if (!s.page && !ct) continue;
      const key = `${s.color}|${s.height}`;
      const m = quad(
        key,
        () => new THREE.MeshPhysicalMaterial({ metalness: 0, roughness: 1, ior: 1.5, depthTest: false, depthWrite: false, side: THREE.FrontSide })
      );
      const mat = m.material;
      if (mat.map !== ct || mat.normalMap !== (ink ? ink.tex : null)) {
        mat.map = ct;
        mat.color.set(ct ? 0xffffff : f.paper);
        mat.normalMap = ink ? ink.tex : null;
        mat.roughnessMap = ink ? ink.rough : null;
        mat.roughness = ink ? 1 : PAPER_ROUGH;
        mat.needsUpdate = true;
      }
      mat.normalScale.set(f.ink[0], f.ink[0]);
      place(m, s.quad, [0, 0, 1, 0, 1, 1, 0, 1], 0);
      m.renderOrder = order++;
      m.userData = { sheet: s, ink: f.ink };
    }
    gl.clear();
    gl.render(scene, camera);
    vignette.material.opacity = f.fx[0] * 0.75;
    if (f.fx[0] > 0) gl.render(over, overCam);
    return complete && envReady === true;
  }

  async function ready(f) {
    await room;
    const waits = [];
    color(f.deskTex, true);
    for (const s of f.sheets) {
      color(s.color, false);
      surface(s.height, s.scale);
    }
    for (const u of [f.deskTex, ...f.sheets.map((s) => s.color)]) {
      const t = u ? textures.get(u) : null;
      if (t && t.promise) waits.push(t.promise);
    }
    await Promise.all(waits);
  }

  function forget(url) {
    const t = textures.get(url);
    if (!t) return;
    for (const k of ["tex", "rough"]) if (t[k]) t[k].dispose();
    textures.delete(url);
    for (const [key, m] of meshes)
      if (key.split("|").includes(url)) {
        m.geometry.dispose();
        m.material.dispose();
        scene.remove(m);
        meshes.delete(key);
      }
  }

  const api = {
    draw,
    ready,
    forget,
    onLoad: null,
    canvas,
    // For the path tracer: what the last frame drew.
    gl,
    scene,
    camera,
    lamp,
    over,
    overCam,
    vignette,
    meshes,
    room: () => roomImage,
  };
  return api;
}

// ------------------------------------------------------------------ on screen

const screens = new Map();
const pending = new Map();
let scheduled = false;

/** Draw a frame on the canvas matching `selector` (at the next animation frame; the latest frame wins). */
export function filmDraw(selector, frame) {
  pending.set(selector, frame);
  if (scheduled) return;
  scheduled = true;
  load().then(() =>
    requestAnimationFrame(() => {
      scheduled = false;
      for (const [sel, f] of pending) {
        const canvas = document.querySelector(sel);
        if (!canvas) continue;
        let r = screens.get(sel);
        if (!r || r.canvas !== canvas) {
          r = renderer(canvas);
          screens.set(sel, r);
        }
        const dpr = Math.min(2, window.devicePixelRatio || 1);
        f.width = Math.round(canvas.clientWidth * dpr);
        f.height = Math.round(canvas.clientHeight * dpr);
        r.onLoad = () => filmDraw(sel, f);
        r.draw(f);
      }
      pending.clear();
    })
  );
}

/** Forget a bitmap's textures everywhere (its object URL is being let go). */
export function filmForget(url) {
  for (const r of screens.values()) r.forget(url);
  if (offscreen) offscreen.forget(url);
}

let offscreen = null;

async function off() {
  await load();
  if (!offscreen) offscreen = renderer(document.createElement("canvas"));
  return offscreen;
}

/** Draw one frame offscreen (every bitmap it names loaded first): a PNG's object URL. */
export async function renderStill(frame) {
  const r = await off();
  await r.ready(frame);
  r.draw(frame);
  const blob = await new Promise((done) => r.canvas.toBlob(done, "image/png"));
  if (!blob) throw new Error("the picture could not be drawn");
  return URL.createObjectURL(blob);
}

/** Film frame after frame offscreen into an MP4 (web/lib/filmgl.js encodeWith). */
export async function encodeFilm(w, h, fps, frames, frameAt, audio, offset, progress) {
  return encodeWith(await off(), w, h, fps, frames, frameAt, audio, offset, progress);
}
