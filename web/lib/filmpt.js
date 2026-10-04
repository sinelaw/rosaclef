// The film's path tracer: a frame of web/src/ui/film.js (GlFrame) built as a
// real 3D scene and rendered by simulating light (three.js with
// three-gpu-pathtracer, web/vendor/pathtracer) — for exported frames and
// videos, where seconds a frame buy a photograph:
//
//  - the paper: each page (and the sharper tiles over it) a plane with its
//    printed color, rough and matte;
//  - the ink: real geometry, raised from the ink's height map (a drop where
//    it pools, nearly flat where it is a fine stroke), black and glossy
//    (wet) or satin (dry) — a dielectric that reflects a few percent of the
//    room, most at grazing angles;
//  - the room: a photographed one (web/vendor/hdri, an artist's workshop with
//    two tall windows, CC0), lighting everything and mirrored in the ink;
//  - the camera: a physical lens with an aperture, focused on the framed
//    music, so what is nearer or farther blurs as in a macro photograph.
//
// Desk units are points: x right, y down the page; in three.js x stays, the
// page's y is z, and up from the desk is y.
//
// Not type-checked (web/types/platform.d.js types what it exports).

import { ENV_ROT, ENV_GAIN } from "./filmgl.js";

let lib = null;
let ctx = null;
const textures = new Map();
const heights = new Map();

const FOV = 32;

async function load() {
  if (!lib) lib = await import("../vendor/pathtracer/pathtracer.mjs");
  return lib;
}

/** An image (an object URL) as decoded pixels. */
async function pixels(url) {
  const img = new Image();
  img.src = url;
  await img.decode();
  const c = document.createElement("canvas");
  c.width = img.naturalWidth;
  c.height = img.naturalHeight;
  const g = c.getContext("2d", { willReadFrequently: true });
  g.drawImage(img, 0, 0);
  return { img, data: g.getImageData(0, 0, c.width, c.height).data, w: c.width, h: c.height };
}

async function colorTexture(url) {
  let t = textures.get(url);
  if (!t) {
    const L = await load();
    const img = new Image();
    img.src = url;
    await img.decode();
    t = new L.Texture(img);
    t.colorSpace = L.SRGBColorSpace;
    t.flipY = false;
    t.needsUpdate = true;
    textures.set(url, t);
  }
  return t;
}

/** The ink of a sheet as geometry: a grid over the sheet, kept where there is ink, raised by its height. */
async function inkGeometry(sheet, lift) {
  const key = `${sheet.height}|${lift}`;
  const hit = heights.get(key);
  if (hit) return hit;
  const L = await load();
  const px = await pixels(sheet.height);
  // About two pixels of the height map a cell, at most 640 cells across.
  const n = Math.max(8, Math.min(640, Math.round(px.w / 2)));
  const m = Math.max(8, Math.round((n * px.h) / px.w));
  const q = sheet.quad;
  const h = new Float32Array((n + 1) * (m + 1));
  for (let j = 0; j <= m; j++)
    for (let i = 0; i <= n; i++) {
      const x = Math.min(px.w - 1, Math.round((i / n) * (px.w - 1)));
      const y = Math.min(px.h - 1, Math.round((j / m) * (px.h - 1)));
      h[j * (n + 1) + i] = 1 - px.data[(y * px.w + x) * 4] / 255;
    }
  const index = new Int32Array((n + 1) * (m + 1)).fill(-1);
  const pos = [];
  const idx = [];
  const vert = (i, j) => {
    const k = j * (n + 1) + i;
    if (index[k] >= 0) return index[k];
    const u = i / n;
    const v = j / m;
    // The sheet's corners: top left, top right, bottom right, bottom left.
    const x = q[0] + (q[2] - q[0]) * u + (q[6] - q[0]) * v;
    const z = q[1] + (q[3] - q[1]) * u + (q[7] - q[1]) * v;
    index[k] = pos.length / 3;
    pos.push(x, 0.004 + h[k] * lift, z);
    return index[k];
  };
  for (let j = 0; j < m; j++)
    for (let i = 0; i < n; i++) {
      const k = j * (n + 1) + i;
      const top = Math.max(h[k], h[k + 1], h[k + n + 1], h[k + n + 2]);
      if (top < 0.02) continue;
      const a = vert(i, j);
      const b = vert(i + 1, j);
      const c = vert(i + 1, j + 1);
      const d = vert(i, j + 1);
      idx.push(a, d, b, b, d, c);
    }
  const geo = new L.BufferGeometry();
  geo.setAttribute("position", new L.BufferAttribute(new Float32Array(pos), 3));
  geo.setIndex(idx);
  geo.computeVertexNormals();
  heights.set(key, geo);
  return geo;
}

/** A flat quad (a page or a tile) with its corners on the desk, `y` above it. */
function quadGeometry(L, q, y) {
  const g = new L.BufferGeometry();
  g.setAttribute("position", new L.BufferAttribute(new Float32Array([q[0], y, q[1], q[2], y, q[3], q[4], y, q[5], q[6], y, q[7]]), 3));
  g.setAttribute("normal", new L.BufferAttribute(new Float32Array([0, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 0]), 3));
  g.setAttribute("uv", new L.BufferAttribute(new Float32Array([0, 0, 1, 0, 1, 1, 0, 1]), 2));
  g.setIndex([0, 3, 1, 1, 3, 2]);
  return g;
}

async function setup(w, h) {
  const L = await load();
  if (ctx && ctx.w === w && ctx.h === h) return ctx;
  if (!ctx) {
    const canvas = document.createElement("canvas");
    const renderer = new L.WebGLRenderer({ canvas, antialias: false, preserveDrawingBuffer: true });
    renderer.toneMapping = L.AgXToneMapping;
    renderer.toneMappingExposure = 1.0;
    const tracer = new L.WebGLPathTracer(renderer);
    tracer.tiles.set(2, 2);
    tracer.filterGlossyFactor = 0.5;
    tracer.minSamples = 1;
    tracer.renderScale = 1;
    const env = await new L.HDRLoader().loadAsync(new URL("../vendor/hdri/artist_workshop_1k.hdr", import.meta.url).href);
    env.mapping = L.EquirectangularReflectionMapping;
    ctx = { canvas, renderer, tracer, env, w: 0, h: 0 };
  }
  ctx.renderer.setSize(w, h, false);
  ctx.w = w;
  ctx.h = h;
  return ctx;
}

/**
 * Path trace a frame: `samples` passes of light (more: less noise), at the
 * frame's size. `progress` hears the fraction done. Resolves to a PNG's object URL.
 */
export async function traceFrame(f, samples, progress) {
  const L = await load();
  const w = Math.round(f.width);
  const h = Math.round(f.height);
  const c = await setup(w, h);
  const scene = new L.Scene();
  scene.environment = c.env;
  scene.background = c.env;
  scene.backgroundBlurriness = 0.15;
  // The windows behind the page and to the left, as on the screen.
  scene.environmentRotation = new L.Euler(0, ENV_ROT, 0);
  scene.backgroundRotation = new L.Euler(0, ENV_ROT, 0);
  scene.environmentIntensity = ENV_GAIN;

  // The desk.
  const dk = f.desk;
  const deskMat = new L.MeshStandardMaterial({ color: new L.Color(f.deskColor), roughness: 0.6 });
  if (f.deskTex) {
    const t = await colorTexture(f.deskTex);
    const tt = t.clone();
    tt.wrapS = L.RepeatWrapping;
    tt.wrapT = L.RepeatWrapping;
    tt.repeat.set((dk[2] - dk[0]) / f.deskTile, (dk[3] - dk[1]) / f.deskTile);
    tt.needsUpdate = true;
    deskMat.map = tt;
    deskMat.color = new L.Color(1, 1, 1);
  }
  scene.add(new L.Mesh(quadGeometry(L, [dk[0], dk[1], dk[2], dk[1], dk[2], dk[3], dk[0], dk[3]], -0.6), deskMat));

  // The paper, and the ink raised over it.
  const relief = f.ink[0];
  const wet = f.ink[1] > 0.01;
  const ink = new L.MeshPhysicalMaterial({
    color: new L.Color(0.012, 0.011, 0.01),
    roughness: wet ? 0.045 + 0.08 * (1 - f.ink[2]) : 0.42,
    metalness: 0,
    ior: 1.5,
    specularIntensity: 1,
  });
  const lift = 0.7 * relief;
  let layer = 0;
  for (const s of f.sheets) {
    if (!s.color) {
      if (s.page) scene.add(new L.Mesh(quadGeometry(L, s.quad, 0), new L.MeshStandardMaterial({ color: new L.Color(f.paper), roughness: 0.85 })));
      continue;
    }
    const paper = new L.MeshStandardMaterial({ map: await colorTexture(s.color), roughness: 0.85 });
    layer = layer + 1;
    scene.add(new L.Mesh(quadGeometry(L, s.quad, s.page ? 0 : 0.0015 + 0.0002 * layer), paper));
    // Ink in relief where the sheet is sharp enough to show it (the tiles).
    if (!s.page && s.height && lift > 0 && s.scale >= 3) scene.add(new L.Mesh(await inkGeometry(s, lift), ink));
  }

  // The camera: as the screen's, with a real lens.
  const [x, y, span, tilt, turn] = f.cam;
  const t = (tilt * Math.PI) / 180;
  const r = (turn * Math.PI) / 180;
  const aspect = w / h;
  const d = span / 2 / Math.tan((FOV * Math.PI) / 360) / aspect;
  const dx = -Math.sin(r);
  const dz = Math.cos(r);
  const cam = new L.PhysicalCamera(FOV, aspect, d * 0.02, d * 40);
  cam.position.set(x + dx * d * Math.sin(t), d * Math.cos(t), y + dz * d * Math.sin(t));
  cam.up.set(-dx, 0, -dz);
  cam.lookAt(new L.Vector3(x, 0, y));
  cam.focusDistance = d;
  // The aperture: scene units are points (a third of a millimetre), so the
  // f-number reads as on a real macro lens a third the size; wider the lower it looks.
  cam.fStop = Math.max(0.6, 3.2 - 0.03 * tilt);
  cam.apertureBlades = 7;
  cam.updateProjectionMatrix();

  c.tracer.setScene(scene, cam);
  for (let i = 0; i < samples * 4; i++) {
    c.tracer.renderSample();
    if (progress && i % 4 === 3) progress((i + 1) / (samples * 4));
    if (c.tracer.samples >= samples) break;
    await new Promise((r) => setTimeout(r, 0));
  }
  const blob = await new Promise((done) => c.canvas.toBlob(done, "image/png"));
  // Geometry of the frame goes with it (the ink's is kept while its tile lives).
  scene.traverse((o) => {
    if (o.isMesh && !Array.from(heights.values()).includes(o.geometry)) o.geometry.dispose();
  });
  return URL.createObjectURL(blob);
}

/** Forget what was made from a bitmap that is being let go. */
export function traceForget(url) {
  const t = textures.get(url);
  if (t) t.dispose();
  textures.delete(url);
  for (const [k, g] of heights) {
    if (k.startsWith(`${url}|`)) {
      g.dispose();
      heights.delete(k);
    }
  }
}
