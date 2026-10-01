// Ink and paper, as SVG: the filter the engraving is drawn through (wet or
// dry ink) and the paper's textures (tiles of noise). The Score view puts them
// on the page; the PDF export draws the same ones onto its pages.
//
// A filter is kept as a small tree of elements, so it can be built into the
// view (score.js) or written out as markup (`markup`).

/** An SVG element: its tag, key (for the view's builder), attributes as name, value pairs, and children. */
/** type El = { tag: String, key: String, attrs: String[], kids: El[] } */

/** function el(tag: String, key: String, attrs: String[], kids: El[]) => El */
function el(tag, key, attrs, kids) {
  return { tag: tag, key: key, attrs: attrs, kids: kids };
}

/** function num(v: Number, digits: Int) => String */
function num(v, digits) {
  const k = Math.pow(10, digits);
  return String(Math.round(v * k) / k);
}

/** XML for an element and its children. */
/** function markup(e: El) => String */
export function markup(e) {
  let out = `<${e.tag}`;
  for (let i = 0; i + 1 < e.attrs.length; i = i + 2) {
    const v = e.attrs[i + 1].replaceAll("&", "&amp;").replaceAll('"', "&quot;").replaceAll("<", "&lt;");
    out = `${out} ${e.attrs[i]}="${v}"`;
  }
  if (e.kids.length === 0) return `${out}/>`;
  return `${out}>${e.kids.map(markup).join("")}</${e.tag}>`;
}

/**
 * Ink on paper, as an SVG filter over the engraving (units are staff spaces;
 * `px` is how many device pixels a staff space covers, so what should stay a
 * hair wide does at any zoom). Two inks, one at a time:
 *  - wet: fresh and glossy — deep, solid ink, its edges rounded a hair as it
 *    bleeds, standing up off the paper over a faint shadow. Where the ink is
 *    thick enough to pool (noteheads, beams, the bowls of the clefs) it bulges,
 *    and its dome catches the light of a window on the upper left; fine strokes
 *    and text stay plain black. The noteheads and dots also catch a crisp
 *    glint (drawn over the music, see `GLOSS`). `gloss` scales how bright the
 *    light is (1 full); `shine` how broad (0 a pin-point speck, 1 most of the dome);
 *  - dry: faded — paper grain nudges the edges a hair (wicking), the ink
 *    spreads, and the outline is drawn full over a slightly translucent,
 *    mottled body: the darker rim a drop of ink leaves as it dries.
 * Colors pass through, so colored passages stay colored.
 */
/** function inkFilter(id: String, wet: Boolean, night: Boolean, gloss: Number, shine: Number, px: Number) => El */
export function inkFilter(id, wet, night, gloss, shine, px) {
  const attrs = ["id", id, "x", "-2%", "y", "-10%", "width", "104%", "height", "120%", "color-interpolation-filters", "sRGB"];
  return el("filter", "ink", attrs, wet ? wetInk(night, gloss, shine, px) : dryInk(px));
}

/** function wetInk(night: Boolean, gloss: Number, shine: Number, px: Number) => El[] */
function wetInk(night, gloss, shine, px) {
  const k = num(gloss, 2);
  // The bleed: a fraction of a staff space, but never more than half a device pixel.
  const bleed = num(Math.min(0.045, 0.5 / Math.max(1, px)), 4);
  return [
    // The ink itself: its edges rounded a hair.
    el("feGaussianBlur", "soft", ["in", "SourceGraphic", "stdDeviation", bleed, "result", "soft"], []),
    el(
      "feComponentTransfer",
      "spread",
      ["in", "soft", "result", "spread"],
      [
        // A smooth curve, not a threshold: the edge keeps its antialiasing.
        el("feFuncA", "a", ["type", "gamma", "amplitude", "1", "exponent", "0.62", "offset", "0"], []),
      ]
    ),
    // Where the ink pools: what is left of it shrunk by a fine stroke's half width
    // (stems, staff text and hairlines vanish), softened at its edge.
    el("feMorphology", "pool", ["in", "SourceAlpha", "operator", "erode", "radius", "0.075", "result", "pool"], []),
    el("feGaussianBlur", "body", ["in", "pool", "stdDeviation", "0.05", "result", "body"], []),
    // Its height: the pooled ink swells into a dome.
    el("feGaussianBlur", "dome", ["in", "SourceAlpha", "stdDeviation", "0.17", "result", "dome"], []),
    // The window's light, mirrored where the dome turns toward it. The tighter
    // the shine, the sharper the reflection (and the brighter, to be seen).
    el(
      "feSpecularLighting",
      "light",
      [
        "in",
        "dome",
        "result",
        "light",
        "surfaceScale",
        "0.35",
        "specularConstant",
        num((2.6 - 1.2 * shine) * (night ? 0.75 : 1), 2),
        "specularExponent",
        num(90 - 60 * shine, 1),
        "lighting-color",
        night ? "#fff3d6" : "#fffdf6",
      ],
      [el("feDistantLight", "sun", ["azimuth", "235", "elevation", "42"], [])]
    ),
    el("feComposite", "lit", ["in", "light", "in2", "body", "operator", "in", "result", "lit"], []),
    el("feColorMatrix", "glint", ["in", "lit", "type", "matrix", "values", `${k} 0 0 0 0  0 ${k} 0 0 0  0 0 ${k} 0 0  0 0 0 ${k} 0`, "result", "glint"], []),
    // The drop's shadow, cast down and to the right on the paper.
    el("feGaussianBlur", "fall", ["in", "SourceAlpha", "stdDeviation", "0.035", "result", "fall"], []),
    el("feOffset", "drop", ["in", "fall", "dx", "0.025", "dy", "0.055", "result", "drop"], []),
    el(
      "feColorMatrix",
      "shade",
      [
        "in",
        "drop",
        "type",
        "matrix",
        "values",
        night ? "0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 0.45 0" : "0 0 0 0 0.16  0 0 0 0 0.12  0 0 0 0 0.06  0 0 0 0.2 0",
        "result",
        "shade",
      ],
      []
    ),
    el(
      "feMerge",
      "merge",
      [],
      [el("feMergeNode", "m0", ["in", "shade"], []), el("feMergeNode", "m1", ["in", "spread"], []), el("feMergeNode", "m2", ["in", "glint"], [])]
    ),
  ];
}

/** function dryInk(px: Number) => El[] */
function dryInk(px) {
  // The spread: a fraction of a staff space, but never more than half a device pixel.
  const spread = num(Math.min(0.035, 0.5 / Math.max(1, px)), 4);
  return [
    el("feTurbulence", "grain", ["type", "fractalNoise", "baseFrequency", "2.4", "numOctaves", "1", "seed", "11", "result", "grain"], []),
    el(
      "feDisplacementMap",
      "wick",
      ["in", "SourceGraphic", "in2", "grain", "scale", "0.07", "xChannelSelector", "R", "yChannelSelector", "G", "result", "wick"],
      []
    ),
    el("feGaussianBlur", "soft", ["in", "wick", "stdDeviation", spread, "result", "soft"], []),
    el(
      "feComponentTransfer",
      "spread",
      ["in", "soft", "result", "spread"],
      [el("feFuncA", "a", ["type", "gamma", "amplitude", "1", "exponent", "0.7", "offset", "0"], [])]
    ),
    el("feMorphology", "core", ["in", "spread", "operator", "erode", "radius", "0.055", "result", "core"], []),
    el("feComposite", "rim", ["in", "spread", "in2", "core", "operator", "out", "result", "rim"], []),
    el("feColorMatrix", "mottle", ["in", "grain", "type", "matrix", "values", "0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0.3 0 0 0 0.78", "result", "mottle"], []),
    el("feComposite", "body", ["in", "spread", "in2", "mottle", "operator", "in", "result", "body"], []),
    el("feMerge", "merge", [], [el("feMergeNode", "m0", ["in", "body"], []), el("feMergeNode", "m1", ["in", "rim"], [])]),
  ];
}

// ------------------------------------------------------------------ paper

/** The paper's textures, each a square tile of noise (`size` pixels) as a data URL. */
/** type Tile = { url: String, size: Int } */
/** type Paper = { color: String, tooth: Tile, mottle: Tile, grain: Tile } */

/** function svgUrl(svg: String) => String */
function svgUrl(svg) {
  return `data:image/svg+xml,${encodeURIComponent(svg)}`;
}

/**
 * The paper's tooth: fibers in relief, lit from the upper left like the ink.
 * Noise tiled seamlessly (made once, then repeated past the edges so the light
 * finds no seam) lights a surface; where it turns from the light it shades in
 * `rgb` (by `shade`), where it turns toward it, it lightens (by `light`).
 */
/** function tooth(size: Int, freq: Number, relief: Number, shade: Number, light: Number, rgb: String) => Tile */
function tooth(size, freq, relief, shade, light, rgb) {
  // The surface lit straight on: sin(50°).
  const flat = 0.766;
  const pad = 16;
  const svg =
    `<svg xmlns='http://www.w3.org/2000/svg' width='${size}' height='${size}'>` +
    `<filter id='t' filterUnits='userSpaceOnUse' x='-${pad}' y='-${pad}' width='${size + 2 * pad}' height='${size + 2 * pad}'>` +
    `<feTurbulence type='fractalNoise' baseFrequency='${freq}' numOctaves='3' seed='7' stitchTiles='stitch' x='0' y='0' width='${size}' height='${size}' result='n'/>` +
    `<feTile in='n' result='t'/>` +
    `<feDiffuseLighting in='t' surfaceScale='${relief}' diffuseConstant='1' lighting-color='#fff' result='l'><feDistantLight azimuth='235' elevation='50'/></feDiffuseLighting>` +
    `<feColorMatrix in='l' result='s' values='0 0 0 0 ${rgb.split(" ")[0]} 0 0 0 0 ${rgb.split(" ")[1]} 0 0 0 0 ${rgb.split(" ")[2]} -${shade} 0 0 0 ${num(shade * flat, 3)}'/>` +
    `<feColorMatrix in='l' result='h' values='0 0 0 0 1 0 0 0 0 1 0 0 0 0 0.97 ${light} 0 0 0 -${num(light * flat, 3)}'/>` +
    `<feMerge><feMergeNode in='s'/><feMergeNode in='h'/></feMerge></filter>` +
    `<rect x='-${pad}' y='-${pad}' width='${size + 2 * pad}' height='${size + 2 * pad}' filter='url(#t)'/></svg>`;
  return { url: svgUrl(svg), size: size };
}

/** Noise as a tint: `rgb`, at the opacity `alpha` (a color matrix row) makes of the noise. */
/** function noise(size: Int, freq: Number, octaves: Int, seed: Int, rgb: String, alpha: String) => Tile */
function noise(size, freq, octaves, seed, rgb, alpha) {
  const ch = rgb.split(" ");
  const svg =
    `<svg xmlns='http://www.w3.org/2000/svg' width='${size}' height='${size}'>` +
    `<filter id='m' x='0' y='0' width='1' height='1'>` +
    `<feTurbulence type='fractalNoise' baseFrequency='${freq}' numOctaves='${octaves}' seed='${seed}' stitchTiles='stitch'/>` +
    `<feColorMatrix values='0 0 0 0 ${ch[0]} 0 0 0 0 ${ch[1]} 0 0 0 0 ${ch[2]} ${alpha}'/></filter>` +
    `<rect width='${size}' height='${size}' filter='url(#m)'/></svg>`;
  return { url: svgUrl(svg), size: size };
}

/** Ivory paper with its tooth, cloudy formation and fine grain. */
/** const PAPER: Paper */
export const PAPER = {
  color: "#fbf7ec",
  tooth: tooth(256, 0.85, 1.4, 0.22, 0.16, "0.36 0.27 0.13"),
  mottle: noise(600, 0.006, 4, 3, "0.55 0.42 0.22", "0.3 0 0 0 -0.1"),
  grain: noise(180, 0.9, 2, 0, "0.45 0.36 0.2", "0 0 0 0.09 0"),
};

/** Night: black lacquer, nearly smooth, with a trace of gold. */
/** const LACQUER: Paper */
export const LACQUER = {
  color: "#15121a",
  tooth: tooth(256, 0.85, 1.4, 0.3, 0.05, "0 0 0"),
  mottle: noise(600, 0.006, 4, 3, "0.83 0.69 0.22", "0.12 0 0 0 -0.05"),
  grain: noise(180, 0.9, 2, 0, "0.83 0.69 0.22", "0 0 0 0.06 0"),
};

// ------------------------------------------------------------------ gloss

/** The wet ink's knobs: how bright its highlights are (1 is full), and how
 * broad (0 a pin-point speck, 1 the whole dome of a notehead). */
export const GLOSS_DEFAULT = 1;
export const GLOSS_MAX = 1.5;
export const SHINE_DEFAULT = 0.75;

/** How strongly a run of the noteheads' glints shows: the crisp glint ("gloss")
 * as bright as the gloss, the broad sheen ("sheen") fading too as the shine tightens to a speck. */
/** function glintOpacity(run: String, gloss: Number, shine: Number) => Number */
export function glintOpacity(run, gloss, shine) {
  const o = run === "sheen" ? 0.16 * Math.min(1, shine / SHINE_DEFAULT) : 0.62;
  return Math.min(1, o * gloss);
}
