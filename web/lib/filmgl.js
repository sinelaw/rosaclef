// The film's renderer (WebGL 2): the desk, the pages and the camera of
// web/src/film.js, drawn as a real 3D scene — for the screen, and frame by
// frame for a video (exportVideo).
//
// The scene is the plane of the desk (desk units: points, x right, y down,
// the camera above it). Each page is a quad with two bitmaps: its color (the
// printed page on paper) and its ink (black where ink lies, softened), from
// which the page shader raises the ink a little off the paper: it lights
// the ink's slopes from the lamp, puts a glossy highlight where the lamp
// reflects toward the camera (wet ink shines, dry ink barely), and lets the
// ink cast a short shadow on the paper. Closer up, sharper bitmaps of a
// system's band are drawn over the page. The picture is drawn into a texture
// and finished in a last pass: depth of field (the far and near edges of a
// leaning picture blurred, from the texture's mipmaps), motion blur for a
// whip, the vignette, the lamp's warmth and a fine grain.
//
// Not type-checked (WebGL is outside inty's library); web/types/platform.d.js
// types what it exports, and GlFrame / GlSheet in web/types/globals.d.js
// describe a frame.

import { inkHeights } from "./inkdrops.js";

const VS = `#version 300 es
in vec2 aPos;
in vec2 aUv;
uniform mat4 uVP;
out vec2 vWorld;
out vec2 vUv;
void main() {
  vWorld = aPos;
  vUv = aUv;
  gl_Position = uVP * vec4(aPos, 0.0, 1.0);
}`;

// Light shared by the desk and the pages: the lamp's pool (a point light
// above the desk) and the spotlight on the framed staves.
const LIGHT = `
uniform vec3 uLight;
uniform vec4 uSpot;
uniform float uSpotAmt;
float lampOn(vec2 p) {
  vec3 d = uLight - vec3(p, 0.0);
  float c = uLight.z / length(d);
  return 0.62 + 0.5 * c * c * c;
}
// The paper keeps its color under the lamp: only a gentle falloff toward the dark.
float paperLamp(vec2 p) {
  vec3 d = uLight - vec3(p, 0.0);
  float c = uLight.z / length(d);
  return 0.86 + 0.16 * c * c * c;
}
float spotOn(vec2 p) {
  if (uSpotAmt <= 0.0) return 1.0;
  float s = length((p - uSpot.xy) / uSpot.zw);
  return mix(1.0, mix(0.5, 1.0, smoothstep(1.4, 0.85, s)), uSpotAmt);
}`;

const DESK_FS = `#version 300 es
precision highp float;
in vec2 vWorld;
in vec2 vUv;
uniform sampler2D uTex;
uniform float uHasTex;
uniform float uTile;
uniform vec3 uColor;
${LIGHT}
out vec4 o;
void main() {
  vec3 c = uHasTex > 0.5 ? texture(uTex, vWorld / uTile).rgb : uColor;
  c *= lampOn(vWorld) * 0.82 * spotOn(vWorld);
  o = vec4(c, 1.0);
}`;

// A soft shadow under a page: the quad grown a little, its alpha falling off toward its edges.
const SHADOW_FS = `#version 300 es
precision highp float;
in vec2 vWorld;
in vec2 vUv;
uniform float uSoft;
out vec4 o;
void main() {
  vec2 d = min(vUv, 1.0 - vUv) / uSoft;
  float a = smoothstep(0.0, 1.0, min(d.x, d.y));
  o = vec4(0.0, 0.0, 0.0, 0.5 * a);
}`;

// The page: paper (its color bitmap, lit by the lamp) and ink raised off it.
// The ink is shaded physically: its height map gives the surface's normal
// (Sobel); the lamp's highlight is a GGX microfacet lobe with Schlick's
// Fresnel; and the ink mirrors the room — a large window behind and to the
// left, a strip light on the right, a warm ceiling, a dark floor — along its
// reflection vector, blurred by its roughness. The room is fixed in the
// world, so the reflections slide over the ink as the camera moves, as they
// would on a wet page. Wet ink is smooth (a sharp mirror), dry ink satin.
const PAGE_FS = `#version 300 es
precision highp float;
in vec2 vWorld;
in vec2 vUv;
uniform sampler2D uColorTex;
uniform sampler2D uInkTex;
uniform float uHasColor;
uniform float uHasInk;
uniform vec2 uTexel;
uniform float uScale;
uniform float uRot;
uniform vec3 uPaper;
uniform vec3 uEye;
uniform float uRelief;
uniform float uGloss;
uniform float uShine;
uniform sampler2D uEnv;
uniform float uHasEnv;
uniform float uEnvRot;
uniform float uEnvGain;
${LIGHT}
out vec4 o;
const float PI = 3.14159265;
const float F0 = 0.045;
float ink(vec2 uv) { return 1.0 - texture(uInkTex, uv).r; }
// The photographed room (an equirectangular HDR image), by direction (z up),
// blurrier the rougher the ink; the made-up one until it has loaded.
vec3 roomOf(vec3 r, float rough);
vec3 env(vec3 r, float rough) {
  if (uHasEnv < 0.5) return roomOf(r, rough);
  // Turned about the vertical: its windows to the left of the page.
  float cr = cos(uEnvRot), sr = sin(uEnvRot);
  vec3 d = vec3(cr * r.x - sr * r.y, sr * r.x + cr * r.y, r.z);
  float u = atan(d.y, d.x) / (2.0 * PI) + 0.5;
  float v = 0.5 - asin(clamp(d.z, -1.0, 1.0)) / PI;
  return textureLod(uEnv, vec2(u, v), rough * 9.0).rgb * uEnvGain;
}
// The room the ink reflects, by direction (z up), its edges softened by roughness.
vec3 roomOf(vec3 r, float rough) {
  float w = 0.03 + rough * 0.6;
  // A dim room: a dark floor, a faintly warm ceiling.
  vec3 c = mix(vec3(0.006, 0.005, 0.004), vec3(0.16, 0.15, 0.13), smoothstep(-0.1, 0.9, r.z));
  // The far wall is a broad window (beyond the top of the page, low in the
  // sky): what a drop of ink mirrors across its top, as a soft band of light.
  vec2 h = normalize(r.xy + vec2(1e-5));
  float facing = smoothstep(0.2 - w, 0.45 + w, -h.y);
  float band = smoothstep(0.02 - w * 0.3, 0.1 + w * 0.3, r.z) * smoothstep(0.62 + w, 0.48 - w * 0.5, r.z);
  c += vec3(14.0, 13.6, 12.8) * facing * band;
  // A smaller, cooler window on the right.
  float side = smoothstep(0.75 - w, 0.85 + w * 0.3, h.x) * smoothstep(0.1 - w * 0.3, 0.2 + w * 0.3, r.z) * smoothstep(0.55 + w, 0.4 - w * 0.5, r.z);
  c += vec3(4.0, 4.3, 4.9) * side;
  return c;
}
// How much of the room a rough dielectric mirrors, seen at nv (Fresnel, with
// the masking and shadowing of its microfacets): Karis's fit of the split-sum
// environment BRDF, as F0 * x + y.
vec2 envBrdf(float nv, float rough) {
  const vec4 c0 = vec4(-1.0, -0.0275, -0.572, 0.022);
  const vec4 c1 = vec4(1.0, 0.0425, 1.04, -0.04);
  vec4 r = rough * c0 + c1;
  float a004 = min(r.x * r.x, exp2(-9.28 * nv)) * r.x + r.y;
  return vec2(-1.04, 1.04) * a004 + r.zw;
}
float ggx(float nh, float a) {
  float a2 = a * a;
  float d = nh * nh * (a2 - 1.0) + 1.0;
  return a2 / (PI * d * d);
}
float smith(float nv, float nl, float a) {
  float k = a * a * 0.5;
  return (nv / (nv * (1.0 - k) + k)) * (nl / (nl * (1.0 - k) + k));
}
void main() {
  vec2 uv = vUv;
  // The relief as large as the ink shows on the screen (pixels a point, here),
  // whichever bitmap draws it: full up close, fading where the strokes are a
  // pixel or less (their slopes would only glint as a silvery fringe). Smooth
  // across the picture, so its near and far parts look alike.
  float onScreen = 1.0 / max(max(length(dFdx(vWorld)), length(dFdy(vWorld))), 1e-6);
  float relief = uRelief * smoothstep(1.8, 3.5, onScreen);
  // The height map's full scale, in points (web/lib/inkdrops.js INK_UNIT).
  float lift = 1.0 * relief;
  float cr = cos(uRot), sr = sin(uRot);
  vec3 v0 = normalize(uEye - vec3(vWorld, 0.0));
  if (uHasInk > 0.5 && relief > 0.0) {
    // Relief mapping: follow the eye's ray down through the ink's height field
    // to where it meets the ink, so a drop shows its true outline from low down.
    vec2 vt = vec2(cr * v0.x + sr * v0.y, -sr * v0.x + cr * v0.y);
    vec2 uvPerPt = uTexel * uScale;
    vec2 span = vt / max(v0.z, 0.12) * lift * uvPerPt;
    vec2 top = uv + span;
    vec2 hit = uv;
    float hp = 1.0;
    // Bare paper all along the ray (the ink's mipmap, as wide as the ray's
    // reach, is white there): nothing to meet, so no march. Most of a close-up.
    float reach = max(length(span / uTexel), 1.0);
    float clear = textureLod(uInkTex, uv + span * 0.5, log2(reach) + 1.0).r;
    for (int i = 1; i <= 32 && clear < 0.998; i++) {
      float k = float(i) / 32.0;
      vec2 q = top - span * k;
      float rayH = 1.0 - k;
      if (ink(q) >= rayH) {
        // Refine between the last step above the ink and this one.
        float lo = k - 1.0 / 32.0, hi = k;
        for (int j = 0; j < 5; j++) {
          float mid = 0.5 * (lo + hi);
          if (ink(top - span * mid) >= 1.0 - mid) hi = mid;
          else lo = mid;
        }
        hit = top - span * hi;
        hp = 0.0;
        break;
      }
    }
    if (hp < 0.5) uv = hit;
  }
  vec3 base = uHasColor > 0.5 ? texture(uColorTex, uv).rgb : uPaper;
  // Up close the paper is smooth: its tooth would look like plaster. Its
  // texture (not the ink's) is kept to what five pixels a point hold, whichever
  // bitmap this is, so the paper looks the same from tile to tile.
  if (uHasColor > 0.5 && uScale > 5.0) {
    vec2 px = vUv * vec2(textureSize(uColorTex, 0));
    float seen = 0.5 * log2(max(dot(dFdx(px), dFdx(px)), dot(dFdy(px), dFdy(px))));
    vec3 smooth_ = textureLod(uColorTex, uv, max(seen, log2(uScale / 5.0))).rgb;
    float paperish = smoothstep(0.55, 0.8, dot(base, vec3(0.299, 0.587, 0.114))) * smoothstep(0.55, 0.8, dot(smooth_, vec3(0.299, 0.587, 0.114)));
    base = mix(base, smooth_, paperish);
  }
  float lamp = paperLamp(vWorld) * mix(1.0, spotOn(vWorld), 0.6);
  vec3 c = base * lamp;
  if (uHasInk > 0.5 && relief > 0.0) {
    vec2 t = uTexel;
    float h0 = ink(uv);
    vec3 p = vec3(vWorld, h0 * lift);
    vec3 l = normalize(uLight - p);
    // Where the ink lies: dark in the color bitmap (the height map's soft skirt is paper).
    float lum = dot(base, vec3(0.299, 0.587, 0.114));
    float onInk = smoothstep(0.62, 0.3, lum) * smoothstep(0.002, 0.02, h0);

    // On the paper: the raised ink casts a short soft shadow, away from the light.
    vec2 toward = normalize(l.xy + 1e-5);
    vec2 tl = vec2(cr * toward.x + sr * toward.y, -sr * toward.x + cr * toward.y);
    vec2 st = tl * uTexel * uScale;
    float blur = log2(max(1.0, uScale * 0.12));
    float occl = 0.0;
    occl += textureLod(uInkTex, uv + st * 0.12, blur).r;
    occl += textureLod(uInkTex, uv + st * 0.24, blur + 0.5).r;
    occl += textureLod(uInkTex, uv + st * 0.38, blur + 1.0).r;
    occl = clamp((1.0 - occl / 3.0) - h0, 0.0, 1.0) * (1.0 - onInk);
    c *= 1.0 - 0.42 * occl * relief;

    // The ink itself (none here: what follows would be weighed by nothing).
    if (onInk > 0.0) {
      // Its slope (Sobel), in points of height a point.
      float a00 = ink(uv + vec2(-t.x, -t.y)), a10 = ink(uv + vec2(0.0, -t.y)), a20 = ink(uv + vec2(t.x, -t.y));
      float a01 = ink(uv + vec2(-t.x, 0.0)), a21 = ink(uv + vec2(t.x, 0.0));
      float a02 = ink(uv + vec2(-t.x, t.y)), a12 = ink(uv + vec2(0.0, t.y)), a22 = ink(uv + vec2(t.x, t.y));
      float gx = (a20 + 2.0 * a21 + a22) - (a00 + 2.0 * a01 + a02);
      float gy = (a02 + 2.0 * a12 + a22) - (a00 + 2.0 * a10 + a20);
      vec2 g = vec2(gx, gy) / 8.0 * uScale * lift;
      g = vec2(cr * g.x - sr * g.y, sr * g.x + cr * g.y);
      vec3 n = normalize(vec3(-g, 1.0));
      vec3 v = normalize(uEye - p);

      // A black body, faintly lit…
      float nl = max(dot(n, l), 0.0);
      float nv = max(dot(n, v), 1e-3);
      c = mix(c, base * (0.5 + 0.5 * nl) * lamp, onInk);
      // …and its gloss: a dielectric's reflection of the room (blurred by
      // roughness, weighed by the split-sum BRDF), and the lamp's GGX highlight
      // when there is no room yet. Wet ink is smooth, dry ink satin.
      float rough = mix(0.45, mix(0.12, 0.04, uShine), clamp(uGloss, 0.0, 1.0));
      float a = rough * rough;
      vec3 r = reflect(-v, n);
      vec2 ab = envBrdf(nv, rough);
      vec3 gloss = env(r, rough) * (F0 * ab.x + ab.y);
      if (uHasEnv < 0.5) {
        vec3 hv = normalize(l + v);
        float nh = max(dot(n, hv), 0.0);
        float vh = max(dot(v, hv), 0.0);
        float fl = F0 + (1.0 - F0) * pow(1.0 - vh, 5.0);
        gloss += vec3(1.0, 0.95, 0.86) * ggx(nh, a) * smith(nv, nl, a) * fl / (4.0 * nv * max(nl, 1e-3)) * nl * 4.0 * lamp;
      }
      c += gloss * onInk;
    }
  }
  o = vec4(c, 1.0);
}`;

// A note as it plays: golden light on it and around it (added to the picture).
const SPARK_FS = `#version 300 es
precision highp float;
in vec2 vWorld;
in vec2 vUv;
uniform float uA;
out vec4 o;
void main() {
  float r = length(vUv * 2.0 - 1.0);
  float core = pow(clamp(1.0 - r * 1.6, 0.0, 1.0), 1.5);
  float halo = pow(clamp(1.0 - r, 0.0, 1.0), 2.5);
  o = vec4(vec3(1.0, 0.68, 0.22) * uA * (0.55 * core + 0.22 * halo), 1.0);
}`;

const POST_VS = `#version 300 es
in vec2 aPos;
out vec2 vUv;
void main() {
  vUv = aPos * 0.5 + 0.5;
  gl_Position = vec4(aPos, 0.0, 1.0);
}`;

const POST_FS = `#version 300 es
precision highp float;
in vec2 vUv;
uniform sampler2D uScene;
uniform mat4 uInvVP;
uniform vec3 uEyeGL;
uniform float uFocusD;
uniform float uCoc;
uniform float uBlur;
uniform float uVignette;
uniform float uAspect;
uniform float uSeed;
uniform vec2 uPx;
out vec4 o;
float rnd(vec2 p) { return fract(sin(dot(p, vec2(12.9898, 78.233)) + uSeed) * 43758.5453); }
void main() {
  // A lens: how far the desk is here (the ray through this pixel meets the
  // desk's plane), and the blur circle of what is that far from the focus.
  vec2 ndc = vUv * 2.0 - 1.0;
  vec4 a = uInvVP * vec4(ndc, -1.0, 1.0);
  vec4 b = uInvVP * vec4(ndc, 1.0, 1.0);
  vec3 p0 = a.xyz / a.w;
  vec3 dir = b.xyz / b.w - p0;
  float coc = 40.0;
  if (abs(dir.z) > 1e-6) {
    float s = -p0.z / dir.z;
    if (s > 0.0) {
      float dist = length(p0 + dir * s - uEyeGL);
      coc = uCoc * abs(1.0 - uFocusD / dist);
    }
  }
  coc = min(coc + uBlur * 24.0, 40.0);
  vec3 c;
  if (coc < 0.6) c = texture(uScene, vUv).rgb;
  else {
    // Gathered over a disc (a golden-angle spiral), from a mipmap that matches its size.
    float lod = max(0.0, log2(coc / 6.0));
    vec3 sum = vec3(0.0);
    float turn = rnd(vUv) * 6.2831;
    for (int i = 0; i < 24; i++) {
      float f = (float(i) + 0.5) / 24.0;
      float ang = float(i) * 2.39996 + turn;
      vec2 off = vec2(cos(ang), sin(ang)) * sqrt(f) * coc * uPx;
      sum += textureLod(uScene, vUv + off, lod).rgb;
    }
    c = sum / 24.0;
  }
  // The lamp's warmth from the upper left, the corners falling into shadow.
  vec2 q = vUv - vec2(0.28, 0.92);
  q.x *= uAspect;
  c += vec3(1.0, 0.92, 0.78) * 0.03 * smoothstep(1.1, 0.0, length(q));
  vec2 v = (vUv - 0.5) * vec2(uAspect, 1.0) / max(1.0, uAspect * 0.85);
  c *= mix(1.0, smoothstep(1.2, 0.35, length(v)), uVignette * 0.75);
  // Highlights roll off instead of clipping (the paper, below 0.98, is left alone).
  c = mix(c, 0.98 + 0.3 * (1.0 - exp(-(c - 0.98) / 0.3)), step(0.98, c));
  c += (rnd(vUv * 731.0) - 0.5) * 0.008;
  o = vec4(c, 1.0);
}`;

// ------------------------------------------------------------------ matrices

function mul(a, b) {
  const o = new Float32Array(16);
  for (let c = 0; c < 4; c++)
    for (let r = 0; r < 4; r++) {
      let s = 0;
      for (let k = 0; k < 4; k++) s += a[k * 4 + r] * b[c * 4 + k];
      o[c * 4 + r] = s;
    }
  return o;
}

function invert(m) {
  const inv = new Float32Array(16);
  inv[0] = m[5] * m[10] * m[15] - m[5] * m[11] * m[14] - m[9] * m[6] * m[15] + m[9] * m[7] * m[14] + m[13] * m[6] * m[11] - m[13] * m[7] * m[10];
  inv[4] = -m[4] * m[10] * m[15] + m[4] * m[11] * m[14] + m[8] * m[6] * m[15] - m[8] * m[7] * m[14] - m[12] * m[6] * m[11] + m[12] * m[7] * m[10];
  inv[8] = m[4] * m[9] * m[15] - m[4] * m[11] * m[13] - m[8] * m[5] * m[15] + m[8] * m[7] * m[13] + m[12] * m[5] * m[11] - m[12] * m[7] * m[9];
  inv[12] = -m[4] * m[9] * m[14] + m[4] * m[10] * m[13] + m[8] * m[5] * m[14] - m[8] * m[6] * m[13] - m[12] * m[5] * m[10] + m[12] * m[6] * m[9];
  inv[1] = -m[1] * m[10] * m[15] + m[1] * m[11] * m[14] + m[9] * m[2] * m[15] - m[9] * m[3] * m[14] - m[13] * m[2] * m[11] + m[13] * m[3] * m[10];
  inv[5] = m[0] * m[10] * m[15] - m[0] * m[11] * m[14] - m[8] * m[2] * m[15] + m[8] * m[3] * m[14] + m[12] * m[2] * m[11] - m[12] * m[3] * m[10];
  inv[9] = -m[0] * m[9] * m[15] + m[0] * m[11] * m[13] + m[8] * m[1] * m[15] - m[8] * m[3] * m[13] - m[12] * m[1] * m[11] + m[12] * m[3] * m[9];
  inv[13] = m[0] * m[9] * m[14] - m[0] * m[10] * m[13] - m[8] * m[1] * m[14] + m[8] * m[2] * m[13] + m[12] * m[1] * m[10] - m[12] * m[2] * m[9];
  inv[2] = m[1] * m[6] * m[15] - m[1] * m[7] * m[14] - m[5] * m[2] * m[15] + m[5] * m[3] * m[14] + m[13] * m[2] * m[7] - m[13] * m[3] * m[6];
  inv[6] = -m[0] * m[6] * m[15] + m[0] * m[7] * m[14] + m[4] * m[2] * m[15] - m[4] * m[3] * m[14] - m[12] * m[2] * m[7] + m[12] * m[3] * m[6];
  inv[10] = m[0] * m[5] * m[15] - m[0] * m[7] * m[13] - m[4] * m[1] * m[15] + m[4] * m[3] * m[13] + m[12] * m[1] * m[7] - m[12] * m[3] * m[5];
  inv[14] = -m[0] * m[5] * m[14] + m[0] * m[6] * m[13] + m[4] * m[1] * m[14] - m[4] * m[2] * m[13] - m[12] * m[1] * m[6] + m[12] * m[2] * m[5];
  inv[3] = -m[1] * m[6] * m[11] + m[1] * m[7] * m[10] + m[5] * m[2] * m[11] - m[5] * m[3] * m[10] - m[9] * m[2] * m[7] + m[9] * m[3] * m[6];
  inv[7] = m[0] * m[6] * m[11] - m[0] * m[7] * m[10] - m[4] * m[2] * m[11] + m[4] * m[3] * m[10] + m[8] * m[2] * m[7] - m[8] * m[3] * m[6];
  inv[11] = -m[0] * m[5] * m[11] + m[0] * m[7] * m[9] + m[4] * m[1] * m[11] - m[4] * m[3] * m[9] - m[8] * m[1] * m[7] + m[8] * m[3] * m[5];
  inv[15] = m[0] * m[5] * m[10] - m[0] * m[6] * m[9] - m[4] * m[1] * m[10] + m[4] * m[2] * m[9] + m[8] * m[1] * m[6] - m[8] * m[2] * m[5];
  const det = m[0] * inv[0] + m[1] * inv[4] + m[2] * inv[8] + m[3] * inv[12];
  for (let i = 0; i < 16; i++) inv[i] /= det || 1;
  return inv;
}

function perspective(fovy, aspect, near, far) {
  const f = 1 / Math.tan(fovy / 2);
  const o = new Float32Array(16);
  o[0] = f / aspect;
  o[5] = f;
  o[10] = (far + near) / (near - far);
  o[11] = -1;
  o[14] = (2 * far * near) / (near - far);
  return o;
}

function lookAt(eye, at, up) {
  let zx = eye[0] - at[0],
    zy = eye[1] - at[1],
    zz = eye[2] - at[2];
  let l = Math.hypot(zx, zy, zz);
  zx /= l;
  zy /= l;
  zz /= l;
  let xx = up[1] * zz - up[2] * zy,
    xy = up[2] * zx - up[0] * zz,
    xz = up[0] * zy - up[1] * zx;
  l = Math.hypot(xx, xy, xz);
  xx /= l;
  xy /= l;
  xz /= l;
  const yx = zy * xz - zz * xy,
    yy = zz * xx - zx * xz,
    yz = zx * xy - zy * xx;
  const o = new Float32Array(16);
  o[0] = xx;
  o[4] = xy;
  o[8] = xz;
  o[1] = yx;
  o[5] = yy;
  o[9] = yz;
  o[2] = zx;
  o[6] = zy;
  o[10] = zz;
  o[12] = -(xx * eye[0] + xy * eye[1] + xz * eye[2]);
  o[13] = -(yx * eye[0] + yy * eye[1] + yz * eye[2]);
  o[14] = -(zx * eye[0] + zy * eye[1] + zz * eye[2]);
  o[15] = 1;
  return o;
}

const FOV = (32 * Math.PI) / 180;
/**
 * The room's turn about the vertical (radians): its windows to the left of the
 * page, on the lamp's side. Glossy ink lit from the side glints where a bead
 * leans toward the light; lit from behind, every wet stroke seen low down
 * mirrors the window along its length (silver).
 */
const ENV_ROT = 0.83;

/**
 * The camera of a frame: in the desk's space (x right, y down, z *into* the
 * desk, so the camera's height is negative z), looking at (x, y), seeing
 * `span` points across, leaning back by `tilt` toward the bottom of the
 * picture, turned by `turn`. Returns the view-projection and the eye in the
 * shading space (z up).
 */
function camera(cam, aspect) {
  const [x, y, span, tilt, turn] = cam;
  const t = (tilt * Math.PI) / 180;
  const r = (turn * Math.PI) / 180;
  const d = span / 2 / Math.tan(FOV / 2) / aspect;
  // The picture's "down" on the desk, and the camera leaning back along it.
  const dx = -Math.sin(r),
    dy = Math.cos(r);
  const eye = [x + dx * d * Math.sin(t), y + dy * d * Math.sin(t), -d * Math.cos(t)];
  const up = [-dx, -dy, 0];
  if (t < 1e-3) {
    up[0] = -dx;
    up[1] = -dy;
  }
  const view = lookAt(eye, [x, y, 0], up);
  const proj = perspective(FOV, aspect, d * 0.02, d * 20);
  const vp = mul(proj, view);
  return { vp, inv: invert(vp), eyeGL: eye, d, eye: [eye[0], eye[1], -eye[2]] };
}

// ------------------------------------------------------------------ the room

/** Decode a Radiance .hdr (RGBE, run-length encoded scanlines): its size and linear RGB floats. */
function parseHdr(buf) {
  const bytes = new Uint8Array(buf);
  let pos = 0;
  const line = () => {
    let s = "";
    while (pos < bytes.length && bytes[pos] !== 10) s += String.fromCharCode(bytes[pos++]);
    pos++;
    return s;
  };
  let l = line();
  while (l !== "") l = line();
  const dims = line().match(/-Y (\d+) \+X (\d+)/);
  if (!dims) throw new Error("unsupported HDR image");
  const h = Number(dims[1]);
  const w = Number(dims[2]);
  const out = new Float32Array(w * h * 3);
  const scan = new Uint8Array(w * 4);
  for (let y = 0; y < h; y++) {
    if (bytes[pos] === 2 && bytes[pos + 1] === 2 && (bytes[pos + 2] & 0x80) === 0) {
      pos += 4;
      for (let c = 0; c < 4; c++) {
        let x = 0;
        while (x < w) {
          let n = bytes[pos++];
          if (n > 128) {
            n -= 128;
            const v = bytes[pos++];
            for (let i = 0; i < n; i++) scan[x++ * 4 + c] = v;
          } else for (let i = 0; i < n; i++) scan[x++ * 4 + c] = bytes[pos++];
        }
      }
    } else {
      for (let x = 0; x < w; x++) for (let c = 0; c < 4; c++) scan[x * 4 + c] = bytes[pos++];
    }
    for (let x = 0; x < w; x++) {
      const e = scan[x * 4 + 3];
      const f = e === 0 ? 0 : Math.pow(2, e - 136);
      const o = (y * w + x) * 3;
      out[o] = scan[x * 4] * f;
      out[o + 1] = scan[x * 4 + 1] * f;
      out[o + 2] = scan[x * 4 + 2] * f;
    }
  }
  return { w, h, data: out };
}

const halfBuf = new Float32Array(1);
const halfInt = new Uint32Array(halfBuf.buffer);
function toHalf(v) {
  halfBuf[0] = v;
  const x = halfInt[0];
  const sign = (x >> 16) & 0x8000;
  const e = ((x >> 23) & 0xff) - 112;
  const m = x & 0x7fffff;
  if (e <= 0) return sign;
  if (e >= 31) return sign | 0x7bff;
  return sign | (e << 10) | (m >> 13);
}

let roomPromise = null;
/** The room's image, decoded once, with its mipmaps (each half the last), for any renderer. */
function roomImage() {
  if (!roomPromise)
    roomPromise = fetch(new URL("../vendor/hdri/artist_workshop_1k.hdr", import.meta.url))
      .then((r) => r.arrayBuffer())
      .then((buf) => {
        let img = parseHdr(buf);
        const levels = [img];
        while (img.w > 1 || img.h > 1) {
          const w = Math.max(1, img.w >> 1);
          const h = Math.max(1, img.h >> 1);
          const d = new Float32Array(w * h * 3);
          for (let y = 0; y < h; y++)
            for (let x = 0; x < w; x++)
              for (let c = 0; c < 3; c++) {
                const x0 = Math.min(img.w - 1, x * 2),
                  x1 = Math.min(img.w - 1, x * 2 + 1);
                const y0 = Math.min(img.h - 1, y * 2),
                  y1 = Math.min(img.h - 1, y * 2 + 1);
                d[(y * w + x) * 3 + c] =
                  (img.data[(y0 * img.w + x0) * 3 + c] +
                    img.data[(y0 * img.w + x1) * 3 + c] +
                    img.data[(y1 * img.w + x0) * 3 + c] +
                    img.data[(y1 * img.w + x1) * 3 + c]) /
                  4;
              }
          img = { w, h, data: d };
          levels.push(img);
        }
        // The room's light in the paper's units: a white page lit by the room
        // alone shows as bright as the lamp shows the paper (1). Its window,
        // mirrored in the ink, is then as much brighter than the paper as it
        // would be in that room, not by a free gain.
        const L = levels[Math.min(levels.length - 1, 3)];
        let sum = 0;
        for (let y = 0; y < L.h; y++) {
          const el = (0.5 - (y + 0.5) / L.h) * Math.PI;
          const up = Math.sin(el);
          if (up <= 0) continue;
          const dA = ((2 * Math.PI) / L.w) * (Math.PI / L.h) * Math.cos(el);
          for (let x = 0; x < L.w; x++) {
            const k = (y * L.w + x) * 3;
            sum += (0.2126 * L.data[k] + 0.7152 * L.data[k + 1] + 0.0722 * L.data[k + 2]) * up * dA;
          }
        }
        return { levels, gain: sum > 0 ? Math.PI / sum : 1 };
      });
  return roomPromise;
}

// ------------------------------------------------------------------ renderer

function compile(gl, vs, fs) {
  const p = gl.createProgram();
  for (const [type, src] of [
    [gl.VERTEX_SHADER, vs],
    [gl.FRAGMENT_SHADER, fs],
  ]) {
    const s = gl.createShader(type);
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s));
    gl.attachShader(p, s);
  }
  gl.bindAttribLocation(p, 0, "aPos");
  gl.bindAttribLocation(p, 1, "aUv");
  gl.linkProgram(p);
  if (!gl.getProgramParameter(p, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(p));
  const u = {};
  const n = gl.getProgramParameter(p, gl.ACTIVE_UNIFORMS);
  for (let i = 0; i < n; i++) {
    const info = gl.getActiveUniform(p, i);
    u[info.name] = gl.getUniformLocation(p, info.name);
  }
  return { p, u };
}

function hexRgb(hex) {
  const h = hex.replace("#", "");
  const v = parseInt(h, 16);
  return [((v >> 16) & 255) / 255, ((v >> 8) & 255) / 255, (v & 255) / 255];
}

/** A renderer on a canvas (on screen or not). */
function renderer(canvas) {
  const gl = canvas.getContext("webgl2", { antialias: true, alpha: false, preserveDrawingBuffer: true });
  if (!gl) return null;
  const progs = {
    desk: compile(gl, VS, DESK_FS),
    shadow: compile(gl, VS, SHADOW_FS),
    page: compile(gl, VS, PAGE_FS),
    spark: compile(gl, VS, SPARK_FS),
    post: compile(gl, POST_VS, POST_FS),
  };
  const buf = gl.createBuffer();
  const vao = gl.createVertexArray();
  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.enableVertexAttribArray(0);
  gl.enableVertexAttribArray(1);
  gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 16, 0);
  gl.vertexAttribPointer(1, 2, gl.FLOAT, false, 16, 8);
  const aniso = gl.getExtension("EXT_texture_filter_anisotropic");
  // Textures by URL: loading, loaded (with their size) or failed.
  const textures = new Map();
  // The room the ink reflects (none until it has loaded).
  let envTex = null;
  let envGain = 1;
  roomImage()
    .then(({ levels, gain }) => {
      envGain = gain;
      const tex = gl.createTexture();
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 2);
      for (let i = 0; i < levels.length; i++) {
        const L = levels[i];
        const half = new Uint16Array(L.w * L.h * 3);
        for (let k = 0; k < half.length; k++) half[k] = toHalf(L.data[k]);
        gl.texImage2D(gl.TEXTURE_2D, i, gl.RGB16F, L.w, L.h, 0, gl.RGB, gl.HALF_FLOAT, half);
      }
      gl.pixelStorei(gl.UNPACK_ALIGNMENT, 4);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      envTex = tex;
      if (api.onLoad) api.onLoad();
    })
    .catch((e) => console.warn("film: no room to reflect", e));
  let target = null;

  /** The ink's heights (web/lib/inkdrops.js), at 16 bits: a half-float texture, its mipmaps made here. */
  function heightTexture(hm) {
    const tex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 2);
    let w = hm.w;
    let h = hm.h;
    let level = new Float32Array(w * h);
    for (let i = 0; i < w * h; i++) level[i] = hm.data[i] / 65535;
    for (let i = 0; ; i++) {
      const half = new Uint16Array(w * h);
      for (let k = 0; k < half.length; k++) half[k] = toHalf(level[k]);
      gl.texImage2D(gl.TEXTURE_2D, i, gl.R16F, w, h, 0, gl.RED, gl.HALF_FLOAT, half);
      if (w === 1 && h === 1) break;
      const w2 = Math.max(1, w >> 1);
      const h2 = Math.max(1, h >> 1);
      const next = new Float32Array(w2 * h2);
      for (let y = 0; y < h2; y++)
        for (let x = 0; x < w2; x++) {
          const x0 = Math.min(w - 1, x * 2),
            x1 = Math.min(w - 1, x * 2 + 1);
          const y0 = Math.min(h - 1, y * 2),
            y1 = Math.min(h - 1, y * 2 + 1);
          next[y * w2 + x] = (level[y0 * w + x0] + level[y0 * w + x1] + level[y1 * w + x0] + level[y1 * w + x1]) / 4;
        }
      level = next;
      w = w2;
      h = h2;
    }
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 4);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    if (aniso) gl.texParameterf(gl.TEXTURE_2D, aniso.TEXTURE_MAX_ANISOTROPY_EXT, Math.min(8, gl.getParameter(aniso.MAX_TEXTURE_MAX_ANISOTROPY_EXT)));
    return tex;
  }

  function texture(url, repeat) {
    if (!url) return null;
    let t = textures.get(url);
    if (!t && url.startsWith("ink:")) {
      const hm = inkHeights(url);
      t = { tex: hm ? heightTexture(hm) : null, w: hm ? hm.w : 0, h: hm ? hm.h : 0, failed: !hm, promise: null };
      textures.set(url, t);
    }
    if (!t) {
      t = { tex: null, w: 0, h: 0, failed: false, promise: null };
      textures.set(url, t);
      const img = new Image();
      img.src = url;
      t.promise = img
        .decode()
        .then(() => {
          const tex = gl.createTexture();
          gl.bindTexture(gl.TEXTURE_2D, tex);
          gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img);
          gl.generateMipmap(gl.TEXTURE_2D);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
          const wrap = repeat ? gl.REPEAT : gl.CLAMP_TO_EDGE;
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, wrap);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, wrap);
          if (aniso) gl.texParameterf(gl.TEXTURE_2D, aniso.TEXTURE_MAX_ANISOTROPY_EXT, Math.min(8, gl.getParameter(aniso.MAX_TEXTURE_MAX_ANISOTROPY_EXT)));
          t.tex = tex;
          t.w = img.naturalWidth;
          t.h = img.naturalHeight;
          if (api.onLoad) api.onLoad();
        })
        .catch(() => {
          t.failed = true;
        });
    }
    return t.tex ? t : null;
  }

  function quad(pts, uvs) {
    // Two triangles: corners 0 1 2, 0 2 3.
    const d = new Float32Array(24);
    const order = [0, 1, 2, 0, 2, 3];
    for (let i = 0; i < 6; i++) {
      const k = order[i];
      d[i * 4] = pts[k * 2];
      d[i * 4 + 1] = pts[k * 2 + 1];
      d[i * 4 + 2] = uvs[k * 2];
      d[i * 4 + 3] = uvs[k * 2 + 1];
    }
    gl.bufferData(gl.ARRAY_BUFFER, d, gl.STREAM_DRAW);
    gl.drawArrays(gl.TRIANGLES, 0, 6);
  }

  function ensureTarget(w, h) {
    if (target && target.w === w && target.h === h) return target;
    if (target) {
      gl.deleteTexture(target.tex);
      gl.deleteFramebuffer(target.fb);
    }
    const tex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texStorage2D(gl.TEXTURE_2D, Math.floor(Math.log2(Math.max(w, h))) + 1, gl.RGBA8, w, h);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR_MIPMAP_LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    const fb = gl.createFramebuffer();
    gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
    gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
    target = { w, h, tex, fb };
    return target;
  }

  function common(prog, f, vp) {
    gl.useProgram(prog.p);
    gl.uniformMatrix4fv(prog.u.uVP, false, vp);
    if (prog.u.uLight) gl.uniform3f(prog.u.uLight, f.light[0], f.light[1], f.light[2]);
    if (prog.u.uSpot) {
      gl.uniform4f(prog.u.uSpot, f.spot[0], f.spot[1], Math.max(1, f.spot[2]), Math.max(1, f.spot[3]));
      gl.uniform1f(prog.u.uSpotAmt, f.spot[4]);
    }
  }

  /** Draw a frame; returns whether every bitmap it names was there. */
  function draw(f) {
    const w = Math.max(1, Math.round(f.width));
    const h = Math.max(1, Math.round(f.height));
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }
    let complete = true;
    const aspect = w / h;
    const { vp, eye, inv, eyeGL, d } = camera(f.cam, aspect);
    const tg = ensureTarget(w, h);
    gl.bindFramebuffer(gl.FRAMEBUFFER, tg.fb);
    gl.viewport(0, 0, w, h);
    gl.clearColor(0.03, 0.025, 0.02, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    gl.disable(gl.DEPTH_TEST);

    // The desk.
    const dk = f.desk;
    const deskTex = texture(f.deskTex, true);
    if (f.deskTex && !deskTex) complete = false;
    common(progs.desk, f, vp);
    gl.uniform1f(progs.desk.u.uHasTex, deskTex ? 1 : 0);
    gl.uniform1f(progs.desk.u.uTile, f.deskTile);
    const dc = hexRgb(f.deskColor);
    gl.uniform3f(progs.desk.u.uColor, dc[0], dc[1], dc[2]);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, deskTex ? deskTex.tex : null);
    gl.uniform1i(progs.desk.u.uTex, 0);
    gl.disable(gl.BLEND);
    quad([dk[0], dk[1], dk[2], dk[1], dk[2], dk[3], dk[0], dk[3]], [0, 0, 1, 0, 1, 1, 0, 1]);

    // The pages' shadows: away from the lamp, soft.
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA);
    common(progs.shadow, f, vp);
    for (const s of f.sheets) {
      if (!s.page) continue;
      const q = s.quad;
      const cx = (q[0] + q[4]) / 2,
        cy = (q[1] + q[5]) / 2;
      const grow = 1.06;
      const ox = (cx - f.light[0]) * 0.012,
        oy = (cy - f.light[1]) * 0.012 + 6;
      const pts = [];
      for (let i = 0; i < 4; i++) pts.push(cx + (q[i * 2] - cx) * grow + ox, cy + (q[i * 2 + 1] - cy) * grow + oy);
      gl.uniform1f(progs.shadow.u.uSoft, 0.06);
      quad(pts, [0, 0, 1, 0, 1, 1, 0, 1]);
    }

    // The pages, then the sharper bands over them.
    gl.disable(gl.BLEND);
    common(progs.page, f, vp);
    const pu = progs.page.u;
    const paper = hexRgb(f.paper);
    gl.uniform3f(pu.uPaper, paper[0], paper[1], paper[2]);
    gl.uniform3f(pu.uEye, eye[0], eye[1], eye[2]);
    gl.uniform1f(pu.uRelief, f.ink[0]);
    gl.uniform1f(pu.uGloss, f.ink[1]);
    gl.uniform1f(pu.uShine, f.ink[2]);
    gl.uniform1i(pu.uColorTex, 0);
    gl.uniform1i(pu.uInkTex, 1);
    gl.uniform1i(pu.uEnv, 2);
    gl.activeTexture(gl.TEXTURE2);
    gl.bindTexture(gl.TEXTURE_2D, envTex);
    gl.uniform1f(pu.uHasEnv, envTex ? 1 : 0);
    // The room turned so its windows are to the left of the page.
    gl.uniform1f(pu.uEnvRot, ENV_ROT);
    gl.uniform1f(pu.uEnvGain, envGain);
    for (const s of f.sheets) {
      const ct = texture(s.color, false);
      const it = texture(s.height, false);
      if ((s.color && !ct) || (s.height && !it)) complete = false;
      if (!s.page && !ct) continue;
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, ct ? ct.tex : null);
      gl.activeTexture(gl.TEXTURE1);
      gl.bindTexture(gl.TEXTURE_2D, it ? it.tex : null);
      gl.uniform1f(pu.uHasColor, ct ? 1 : 0);
      gl.uniform1f(pu.uHasInk, it ? 1 : 0);
      gl.uniform2f(pu.uTexel, it ? 1 / it.w : 0.001, it ? 1 / it.h : 0.001);
      gl.uniform1f(pu.uScale, s.scale);
      gl.uniform1f(pu.uRot, (s.rot * Math.PI) / 180);
      quad(s.quad, [0, 0, 1, 0, 1, 1, 0, 1]);
    }

    // Notes as they play, multiplied into the paper.
    if (f.sparks.length > 0) {
      gl.enable(gl.BLEND);
      gl.blendFunc(gl.ONE, gl.ONE);
      common(progs.spark, f, vp);
      for (let i = 0; i + 3 < f.sparks.length; i += 4) {
        const [x, y, r, a] = f.sparks.slice(i, i + 4);
        gl.uniform1f(progs.spark.u.uA, a);
        quad([x - r, y - r, x + r, y - r, x + r, y + r, x - r, y + r], [0, 0, 1, 0, 1, 1, 0, 1]);
      }
      gl.disable(gl.BLEND);
    }

    // The finish, onto the canvas.
    gl.bindTexture(gl.TEXTURE_2D, tg.tex);
    gl.generateMipmap(gl.TEXTURE_2D);
    gl.bindFramebuffer(gl.FRAMEBUFFER, null);
    gl.viewport(0, 0, w, h);
    const po = progs.post;
    gl.useProgram(po.p);
    gl.activeTexture(gl.TEXTURE0);
    gl.bindTexture(gl.TEXTURE_2D, tg.tex);
    gl.uniform1i(po.u.uScene, 0);
    gl.uniformMatrix4fv(po.u.uInvVP, false, inv);
    gl.uniform3f(po.u.uEyeGL, eyeGL[0], eyeGL[1], eyeGL[2]);
    gl.uniform1f(po.u.uFocusD, d);
    // The lens's blur circle, in pixels, for what is twice as far as the focus.
    gl.uniform1f(po.u.uCoc, f.fx[1] * h * 0.03);
    gl.uniform1f(po.u.uBlur, f.cam[5]);
    gl.uniform1f(po.u.uVignette, f.fx[0]);
    gl.uniform1f(po.u.uAspect, aspect);
    gl.uniform1f(po.u.uSeed, f.seed);
    gl.uniform2f(po.u.uPx, 1 / w, 1 / h);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 0, 0, 1, -1, 0, 0, 1, 1, 0, 0, -1, -1, 0, 0, 1, 1, 0, 0, -1, 1, 0, 0]), gl.STREAM_DRAW);
    gl.drawArrays(gl.TRIANGLES, 0, 6);
    return complete;
  }

  /** Wait for the bitmaps a frame names. */
  async function ready(f) {
    const urls = [f.deskTex];
    for (const s of f.sheets) urls.push(s.color, s.height);
    for (const u of urls) {
      if (!u) continue;
      texture(u, u === f.deskTex);
      const t = textures.get(u);
      if (t && t.promise) await t.promise;
    }
  }

  function forget(url) {
    const t = textures.get(url);
    if (t && t.tex) gl.deleteTexture(t.tex);
    textures.delete(url);
  }

  const api = { draw, ready, forget, onLoad: null, canvas };
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
  requestAnimationFrame(() => {
    scheduled = false;
    for (const [sel, f] of pending) {
      const canvas = document.querySelector(sel);
      if (!canvas) continue;
      let r = screens.get(sel);
      if (!r || r.canvas !== canvas) {
        try {
          r = renderer(canvas);
        } catch (e) {
          console.error("film renderer", e);
          r = null;
        }
        if (!r) continue;
        screens.set(sel, r);
      }
      const dpr = Math.min(2, window.devicePixelRatio || 1);
      f.width = Math.round(canvas.clientWidth * dpr);
      f.height = Math.round(canvas.clientHeight * dpr);
      r.onLoad = () => filmDraw(sel, f);
      r.draw(f);
    }
    pending.clear();
  });
}

/** Forget a bitmap's texture everywhere (its object URL is being let go). */
export function filmForget(url) {
  for (const r of screens.values()) r.forget(url);
  if (offscreen) offscreen.forget(url);
}

// ------------------------------------------------------------------ stills

/** Draw one frame offscreen (every bitmap it names loaded first): a PNG's object URL. */
export async function renderStill(frame) {
  if (!offscreen) {
    const canvas = document.createElement("canvas");
    offscreen = renderer(canvas);
    if (!offscreen) throw new Error("WebGL 2 is not available");
  }
  await offscreen.ready(frame);
  offscreen.draw(frame);
  const blob = await new Promise((done) => offscreen.canvas.toBlob(done, "image/png"));
  if (!blob) throw new Error("the picture could not be drawn");
  return URL.createObjectURL(blob);
}

// ------------------------------------------------------------------ video

let offscreen = null;

/** The best codecs this browser encodes to MP4: H.264 and AAC where it can, VP9 and Opus otherwise. */
async function codecs(w, h, fps) {
  const video = [
    { codec: "avc1.640033", mux: "avc" },
    { codec: "avc1.4d0033", mux: "avc" },
    { codec: "vp09.00.41.08", mux: "vp9" },
    { codec: "av01.0.08M.08", mux: "av1" },
  ];
  let v = null;
  for (const c of video) {
    const cfg = { codec: c.codec, width: w, height: h, bitrate: Math.round(w * h * fps * 0.1), framerate: fps };
    if (c.mux === "avc") cfg.avc = { format: "avc" };
    try {
      if ((await VideoEncoder.isConfigSupported(cfg)).supported) {
        v = { cfg, mux: c.mux };
        break;
      }
    } catch (e) {}
  }
  let a = null;
  for (const c of [
    { codec: "mp4a.40.2", mux: "aac" },
    { codec: "opus", mux: "opus" },
  ]) {
    const cfg = { codec: c.codec, sampleRate: 48000, numberOfChannels: 2, bitrate: 192000 };
    try {
      if (typeof AudioEncoder !== "undefined" && (await AudioEncoder.isConfigSupported(cfg)).supported) {
        a = { cfg, mux: c.mux };
        break;
      }
    } catch (e) {}
  }
  return { v, a };
}

/** Resample decoded audio (one or two channels) to 48 kHz stereo. */
function resample(audio) {
  const n = Math.round((audio.channels[0].length * 48000) / audio.sampleRate);
  const out = [new Float32Array(n), new Float32Array(n)];
  const k = audio.sampleRate / 48000;
  for (let c = 0; c < 2; c++) {
    const src = audio.channels[Math.min(c, audio.channels.length - 1)];
    for (let i = 0; i < n; i++) {
      const x = i * k;
      const j = Math.floor(x);
      const f = x - j;
      out[c][i] = (src[j] ?? 0) * (1 - f) + (src[j + 1] ?? 0) * f;
    }
  }
  return out;
}

/**
 * Encode a film to an MP4: `frames` frames of `w` by `h` at `fps`, each made
 * by `frameAt(i)` (a promise, so its bitmaps can be drawn first), with the
 * decoded mixdown from `offset` seconds as its sound (or none: an empty
 * channel list). `progress`
 * hears the fraction done. Resolves to the file's object URL and its codecs.
 */
export async function encodeFilm(w, h, fps, frames, frameAt, audio, offset, progress) {
  if (!offscreen) {
    const canvas = document.createElement("canvas");
    offscreen = renderer(canvas);
    if (!offscreen) throw new Error("WebGL 2 is not available");
  }
  return encodeWith(offscreen, w, h, fps, frames, frameAt, audio, offset, progress);
}

/** encodeFilm, the frames drawn by `painter` ({ ready(frame), draw(frame), canvas }): this renderer's, or another's (web/lib/filmthree.js). */
export async function encodeWith(painter, w, h, fps, frames, frameAt, audio, offset, progress) {
  if (typeof VideoEncoder === "undefined") throw new Error("this browser cannot encode video (WebCodecs)");
  const { Muxer, ArrayBufferTarget } = await import("../vendor/mp4-muxer/mp4-muxer.mjs");
  const W = Math.round(w / 2) * 2;
  const H = Math.round(h / 2) * 2;
  const { v, a } = await codecs(W, H, fps);
  if (!v) throw new Error("this browser has no video encoder for MP4");
  const sound = audio.channels.length > 0 && a !== null;
  const target = new ArrayBufferTarget();
  const muxer = new Muxer({
    target,
    video: { codec: v.mux, width: W, height: H, frameRate: fps },
    audio: sound ? { codec: a.mux, numberOfChannels: 2, sampleRate: 48000 } : undefined,
    fastStart: "in-memory",
    firstTimestampBehavior: "offset",
  });
  let failure = null;
  const venc = new VideoEncoder({
    output: (chunk, meta) => muxer.addVideoChunk(chunk, meta),
    error: (e) => (failure = e),
  });
  venc.configure(v.cfg);
  for (let i = 0; i < frames; i++) {
    if (failure) throw failure;
    const f = await frameAt(i);
    f.width = W;
    f.height = H;
    await painter.ready(f);
    // A painter may take its time over a frame (the path tracer's samples).
    await painter.draw(f);
    const frame = new VideoFrame(painter.canvas, { timestamp: Math.round((i * 1e6) / fps), duration: Math.round(1e6 / fps) });
    venc.encode(frame, { keyFrame: i % (fps * 2) === 0 });
    frame.close();
    while (venc.encodeQueueSize > 4) await new Promise((r) => setTimeout(r, 1));
    if (progress) progress((i + 1) / frames);
  }
  await venc.flush();
  venc.close();
  if (sound) {
    const pcm = resample(audio);
    const aenc = new AudioEncoder({
      output: (chunk, meta) => muxer.addAudioChunk(chunk, meta),
      error: (e) => (failure = e),
    });
    aenc.configure(a.cfg);
    const step = 4800;
    const skip = Math.max(0, Math.round(offset * 48000));
    const total = Math.max(0, Math.min(pcm[0].length - skip, Math.round((frames / fps) * 48000)));
    for (let s = 0; s < total; s += step) {
      const n = Math.min(step, total - s);
      const data = new Float32Array(n * 2);
      data.set(pcm[0].subarray(skip + s, skip + s + n), 0);
      data.set(pcm[1].subarray(skip + s, skip + s + n), n);
      const ad = new AudioData({
        format: "f32-planar",
        sampleRate: 48000,
        numberOfFrames: n,
        numberOfChannels: 2,
        timestamp: Math.round((s * 1e6) / 48000),
        data,
      });
      aenc.encode(ad);
      ad.close();
    }
    await aenc.flush();
    aenc.close();
  }
  if (failure) throw failure;
  muxer.finalize();
  const blob = new Blob([target.buffer], { type: "video/mp4" });
  return { url: URL.createObjectURL(blob), codecs: `${v.mux.toUpperCase()}${sound ? ` + ${a.mux.toUpperCase()}` : ""}` };
}
