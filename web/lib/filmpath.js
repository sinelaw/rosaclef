// The film path traced: a frame of the three.js renderer (web/lib/filmthree.js)
// rendered by simulating light (three-gpu-pathtracer, on the GPU; vendored in
// web/vendor/pathtracer) — for saved frames and exported videos, where seconds
// a frame buy a photograph. Chosen with ?renderer=trace (web/lib/platform.js);
// the screen shows the three.js renderer meanwhile.
//
// What the light meets: the desk; the pages and the sharper tiles over them
// (matte paper); and, on the close tiles, the ink as real geometry raised from
// its heights (web/lib/inkdrops.js) — glossy black beads that cast their
// shadows, stand in silhouette and mirror the paper around them and the room.
// Farther tiles keep their ink as a normal map. The room (the HDR image) and
// the lamp light it all.
//
// ?samples=N sets the samples a pixel (256); ?crop=x,y,w,h renders only that
// part of the frame (a close look at a note, quickly).
//
// Not type-checked (three.js is outside inty's library).

import { renderer, load, three } from "./filmthree.js";
import { inkHeights } from "./inkdrops.js";
import { encodeWith } from "./filmgl.js";

/** Tiles this sharp (pixels a point) and sharper get raised ink; the step between grid points (pixels) by sharpness. */
const RAISED_FROM = 8;

let lib = null;
let painter = null;

function params() {
  const q = new URLSearchParams(location.search);
  const samples = Math.max(1, Number(q.get("samples")) || 256);
  const crop = (q.get("crop") || "")
    .split(",")
    .map(Number)
    .filter((v) => Number.isFinite(v));
  return { samples, crop: crop.length === 4 ? crop : null };
}

/** The ink of a tile as a mesh: a grid over its quad lifted by the drops' heights (points). */
function raised(T, sheet, relief, lift) {
  const hm = inkHeights(sheet.height);
  if (!hm) return null;
  const step = sheet.scale >= 20 ? 2 : 4;
  const nx = Math.floor((hm.w - 1) / step) + 1;
  const ny = Math.floor((hm.h - 1) / step) + 1;
  const q = sheet.quad;
  const pos = new Float32Array(nx * ny * 3);
  const uv = new Float32Array(nx * ny * 2);
  for (let j = 0; j < ny; j++)
    for (let i = 0; i < nx; i++) {
      const px = Math.min(hm.w - 1, i * step);
      const py = Math.min(hm.h - 1, j * step);
      const u = px / (hm.w - 1);
      const v = py / (hm.h - 1);
      // The quad's corners: top left, top right, bottom right, bottom left.
      const x = q[0] * (1 - u) * (1 - v) + q[2] * u * (1 - v) + q[4] * u * v + q[6] * (1 - u) * v;
      const y = q[1] * (1 - u) * (1 - v) + q[3] * u * (1 - v) + q[5] * u * v + q[7] * (1 - u) * v;
      const h = (1 - hm.data[py * hm.w + px] / 65535) * relief;
      const k = j * nx + i;
      pos[k * 3] = x;
      pos[k * 3 + 1] = lift + h;
      pos[k * 3 + 2] = y;
      uv[k * 2] = u;
      uv[k * 2 + 1] = v;
    }
  const index = new Uint32Array((nx - 1) * (ny - 1) * 6);
  let n = 0;
  for (let j = 0; j < ny - 1; j++)
    for (let i = 0; i < nx - 1; i++) {
      const a = j * nx + i;
      const b = a + 1;
      const c = a + nx + 1;
      const d = a + nx;
      // Wound to face up, as the flat sheets are.
      index[n++] = a;
      index[n++] = c;
      index[n++] = b;
      index[n++] = a;
      index[n++] = d;
      index[n++] = c;
    }
  const geo = new T.BufferGeometry();
  geo.setAttribute("position", new T.BufferAttribute(pos, 3));
  geo.setAttribute("uv", new T.BufferAttribute(uv, 2));
  geo.setIndex(new T.BufferAttribute(index, 1));
  geo.computeVertexNormals();
  return geo;
}

/** Whether a quad (desk points) shows in the camera's picture at all. */
function inView(T, camera, q) {
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  const v = new T.Vector3();
  for (let i = 0; i < 4; i++) {
    v.set(q[i * 2], 0, q[i * 2 + 1]).applyMatrix4(camera.matrixWorldInverse);
    if (v.z > -camera.near) return true;
    v.applyMatrix4(camera.projectionMatrix);
    x0 = Math.min(x0, v.x);
    y0 = Math.min(y0, v.y);
    x1 = Math.max(x1, v.x);
    y1 = Math.max(y1, v.y);
  }
  return x1 >= -1.05 && x0 <= 1.05 && y1 >= -1.05 && y0 <= 1.05;
}

/** A painter ({ ready, draw, canvas }) that path traces frames. */
async function tracer() {
  await load();
  if (!lib) lib = await import("../vendor/pathtracer/pathtracer.js");
  if (painter) return painter;
  const T = three();
  const r = renderer(document.createElement("canvas"));
  const pt = new lib.WebGLPathTracer(r.gl);
  pt.renderDelay = 0;
  pt.fadeDuration = 0;
  pt.minSamples = 1;
  pt.rasterizeScene = false;
  pt.bounces = 6;
  pt.filterGlossyFactor = 0.25;
  pt.tiles.set(2, 2);
  async function draw(f) {
    const { samples, crop } = params();
    // The three.js renderer lays the frame out (its scene, camera and lamp).
    r.draw(f);
    const scene = new T.Scene();
    scene.background = new T.Color(0x080605);
    scene.environment = r.room();
    scene.environmentIntensity = r.scene.environmentIntensity;
    scene.environmentRotation.copy(r.scene.environmentRotation);
    const lamp = r.lamp.clone();
    scene.add(lamp);
    const camera = r.camera.clone();
    const W = Math.round(f.width);
    const H = Math.round(f.height);
    if (crop) camera.setViewOffset(W, H, crop[0], crop[1], crop[2], crop[3]);
    camera.updateProjectionMatrix();
    camera.updateMatrixWorld();
    camera.matrixWorldInverse.copy(camera.matrixWorld).invert();
    r.gl.setSize(crop ? crop[2] : W, crop ? crop[3] : H, false);
    // What the light meets: each opaque quad as drawn, the later a hair above the
    // earlier (the tiles over their page); the close tiles' ink raised.
    const made = [];
    let layer = 0;
    for (const m of [...r.meshes.values()].sort((a, b) => a.renderOrder - b.renderOrder)) {
      if (!m.visible || m.material.transparent) continue;
      layer++;
      const lift = layer * 0.002;
      const s = m.userData.sheet;
      const q = s ? s.quad : null;
      if (q && !inView(T, camera, q)) continue;
      let mesh;
      if (s && s.height && s.scale >= RAISED_FROM && inkHeights(s.height)) {
        const geo = raised(T, s, m.userData.ink[0], lift);
        const mat = m.material.clone();
        mat.normalMap = null;
        mesh = new T.Mesh(geo, mat);
        made.push(geo, mat);
      } else {
        mesh = new T.Mesh(m.geometry, m.material);
        mesh.position.y = lift;
      }
      scene.add(mesh);
    }
    pt.setScene(scene, camera);
    pt.reset();
    while (pt.samples < samples) {
      pt.renderSample();
      // Let the page breathe (and the shaders compile) between samples.
      await new Promise((done) => setTimeout(done, 0));
    }
    // The vignette over the picture.
    if (f.fx[0] > 0 && !crop) {
      r.vignette.material.opacity = f.fx[0] * 0.75;
      r.gl.render(r.over, r.overCam);
    }
    for (const x of made) x.dispose();
    return true;
  }
  painter = { ready: (f) => r.ready(f), draw, canvas: r.canvas };
  return painter;
}

/** Path trace one frame (every bitmap it names loaded first): a PNG's object URL. */
export async function renderStill(frame) {
  const p = await tracer();
  await p.ready(frame);
  await p.draw(frame);
  const blob = await new Promise((done) => p.canvas.toBlob(done, "image/png"));
  if (!blob) throw new Error("the picture could not be drawn");
  return URL.createObjectURL(blob);
}

/** Path trace a film frame after frame into an MP4 (web/lib/filmgl.js encodeWith). */
export async function encodeFilm(w, h, fps, frames, frameAt, audio, offset, progress) {
  return encodeWith(await tracer(), w, h, fps, frames, frameAt, audio, offset, progress);
}
