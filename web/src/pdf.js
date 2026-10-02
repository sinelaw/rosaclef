// Sheet music as a PDF: the score engraved again for a printed page (A4 or
// US Letter), paginated, and written as PDF objects.
//
// Two kinds. The plain one is all vector: the music glyphs are drawn from Bravura's outlines
// (`d` in smufl.js), each defined once as a form and placed wherever it
// appears, so no font is embedded and every viewer shows the same shapes.
// Text uses the PDF standard Times faces. The platform layer compresses
// the streams and writes the file (downloadPdf in web/lib/platform.js).
//
// The other is drawn as on screen: each page as SVG (pageSvg), on textured
// paper and through the ink filter (ink.js), which the platform layer turns
// into an image per page (downloadImagePdf).

import { G, GLYPHS } from "./smufl.js";
import { engrave, GLOSS, SHEEN } from "./engrave.js";
import { inkFilter, markup, glintOpacity, PAPER } from "./ink.js";

/** What heads the first page. `bpm` 0: no tempo mark. */
/** type PdfInfo = { title: String, subtitle: String, author: String, bpm: Number } */
/** Width of a text in a standard font (Times-Roman, Times-Italic, Times-Bold, Times-BoldItalic) at size 1. */
/** type Measurer = (String, String) => Number */

const MM = 72 / 25.4;
/** The ink: warm black, as on screen. */
const INK = "0.106 0.090 0.071";
const STAFF = "0.18 0.16 0.14";

/** function n(v: Number) => String */
function n(v) {
  return String(Math.round(v * 1000) / 1000);
}

// ------------------------------------------------------------------ paths

/** SVG path data (absolute M L H V C Q Z, as in the engraving and the glyph outlines) as PDF path operators. */
/** function pathOps(d: String) => String */
export function pathOps(d) {
  /** const out: String[] */
  const out = [];
  const tokens = d.match(/[MLHVCQZ]|-?[0-9.]+(?:e-?[0-9]+)?/g) ?? [];
  let cmd = "";
  let i = 0;
  let x = 0;
  let y = 0;
  /** function next() => Number */
  function next() {
    const v = Number(tokens[i]);
    i = i + 1;
    return v;
  }
  while (i < tokens.length) {
    const t = tokens[i];
    if (t === "M" || t === "L" || t === "H" || t === "V" || t === "C" || t === "Q" || t === "Z") {
      cmd = t;
      i = i + 1;
      if (t === "Z") {
        out.push("h");
        continue;
      }
    }
    if (cmd === "M" || cmd === "L") {
      x = next();
      y = next();
      out.push(`${n(x)} ${n(y)} ${cmd === "M" ? "m" : "l"}`);
      // Pairs after a moveto are linetos.
      if (cmd === "M") cmd = "L";
    } else if (cmd === "H") {
      x = next();
      out.push(`${n(x)} ${n(y)} l`);
    } else if (cmd === "V") {
      y = next();
      out.push(`${n(x)} ${n(y)} l`);
    } else if (cmd === "C") {
      const x1 = next();
      const y1 = next();
      const x2 = next();
      const y2 = next();
      x = next();
      y = next();
      out.push(`${n(x1)} ${n(y1)} ${n(x2)} ${n(y2)} ${n(x)} ${n(y)} c`);
    } else if (cmd === "Q") {
      const qx = next();
      const qy = next();
      const ex = next();
      const ey = next();
      // A quadratic as a cubic.
      out.push(`${n(x + ((qx - x) * 2) / 3)} ${n(y + ((qy - y) * 2) / 3)} ${n(ex + ((qx - ex) * 2) / 3)} ${n(ey + ((qy - ey) * 2) / 3)} ${n(ex)} ${n(ey)} c`);
      x = ex;
      y = ey;
    } else i = i + 1;
  }
  return out.join("\n");
}

// ------------------------------------------------------------------ text

const WIN = [
  { c: "€", b: 128 },
  { c: "…", b: 133 },
  { c: "Š", b: 138 },
  { c: "Œ", b: 140 },
  { c: "Ž", b: 142 },
  { c: "‘", b: 145 },
  { c: "’", b: 146 },
  { c: "“", b: 147 },
  { c: "”", b: 148 },
  { c: "•", b: 149 },
  { c: "–", b: 150 },
  { c: "—", b: 151 },
  { c: "™", b: 153 },
  { c: "š", b: 154 },
  { c: "œ", b: 156 },
  { c: "ž", b: 158 },
  { c: "Ÿ", b: 159 },
];

/** A PDF string in the standard fonts' encoding (WinAnsi); ♭ and ♯ are spelled out. */
/** function pdfString(s: String) => String */
export function pdfString(s) {
  const text = s.replaceAll("♭", "-flat").replaceAll("♯", "-sharp");
  let out = "(";
  for (const ch of text.split("")) {
    const code = ch.charCodeAt(0);
    if (ch === "(" || ch === ")" || ch === "\\") out = `${out}\\${ch}`;
    else if (code >= 32 && code < 127) out = `${out}${ch}`;
    else {
      const w = WIN.find((e) => e.c === ch);
      const b = w ? w.b : code >= 160 && code <= 255 ? code : 63;
      // Three octal digits.
      out = `${out}\\${Math.floor(b / 64)}${Math.floor(b / 8) % 8}${b % 8}`;
    }
  }
  return `${out})`;
}

/** The text a PDF string shows, for measuring (♭ and ♯ spelled out the same way). */
/** function shown(s: String) => String */
function shown(s) {
  return s.replaceAll("♭", "-flat").replaceAll("♯", "-sharp");
}

const FONTS = ["Times-Roman", "Times-Italic", "Times-Bold", "Times-BoldItalic"];

/** function fontRef(face: String) => String */
function fontRef(face) {
  return `/F${Math.max(0, FONTS.indexOf(face)) + 1}`;
}

/** Text at (x, y) on the page (points, y up), anchored "start", "middle" or "end". */
/** function textAt(m: Measurer, face: String, size: Number, x: Number, y: Number, s: String, anchor: String, color: String) => String */
function textAt(m, face, size, x, y, s, anchor, color) {
  const w = m(face, shown(s)) * size;
  const x0 = anchor === "middle" ? x - w / 2 : anchor === "end" ? x - w : x;
  return `${color} rg BT ${fontRef(face)} ${n(size)} Tf ${n(x0)} ${n(y)} Td ${pdfString(s)} Tj ET`;
}

// ------------------------------------------------------------------ colors

/** function rgb(hex: String, scale: Number) => String */
function rgb(hex, scale) {
  const h = hex.startsWith("#") ? hex.slice(1) : hex;
  const v = parseInt(h, 16);
  if (!(v >= 0)) return INK;
  const r = ((v >> 16) & 255) / 255;
  const g = ((v >> 8) & 255) / 255;
  const b = (v & 255) / 255;
  return `${n(r * scale)} ${n(g * scale)} ${n(b * scale)}`;
}

// ------------------------------------------------------------------ layout

/** A system placed on a page: its index and the y (points from the page top) of its top. */
/** type OnPage = { sys: Int, top: Number } */

/** A score laid out on pages: their size (points), margins, the staff space, the engraving, and its systems page by page. */
/** type PdfLayout = { w: Number, h: Number, left: Number, top: Number, bottom: Number, sp: Number, page: Page, pages: OnPage[][] } */

/** Engrave a score for a printed page (A4 or US Letter) and share its systems out among pages. */
/** function pdfLayout(sc: Score, paper: String, hideEmpty: Boolean) => PdfLayout */
export function pdfLayout(sc, paper, hideEmpty) {
  const W = paper === "letter" ? 612 : 595.28;
  const H = paper === "letter" ? 792 : 841.89;
  const left = 17 * MM;
  const top = 15 * MM;
  const bottom = 17 * MM;
  // The staff size engravers pick for the number of staves (7 mm for a part, smaller for a full score).
  const staves = sc.staves.length;
  const staffMm = staves <= 2 ? 7 : staves <= 4 ? 6.2 : staves <= 8 ? 5.4 : 4.6;
  const sp = (staffMm / 4) * MM;
  const page = engrave(sc, { width: (W - 2 * left) / sp, hideEmpty: hideEmpty });
  const head = 27 * MM;
  const gap = 3 * sp;

  // Pages of systems, the first one under the title.
  /** const pages: OnPage[][] */
  const pages = [[]];
  let y = top + head;
  for (let i = 0; i < page.systems.length; i++) {
    const h = page.systems[i].height * sp;
    const cur = pages[pages.length - 1];
    if (cur.length > 0 && y + h > H - bottom) {
      pages.push([]);
      y = top;
    }
    pages[pages.length - 1].push({ sys: i, top: y });
    y = y + h + gap;
  }
  // Full pages spread their systems down to the bottom margin (at most a few spaces more apart).
  for (let p = 0; p + 1 < pages.length; p++) {
    const list = pages[p];
    if (list.length < 2) continue;
    const last = list[list.length - 1];
    const spare = H - bottom - (last.top + page.systems[last.sys].height * sp);
    const extra = Math.min(spare / (list.length - 1), 6 * sp);
    for (let k = 1; k < list.length; k++) list[k].top = list[k].top + extra * k;
  }
  return { w: W, h: H, left: left, top: top, bottom: bottom, sp: sp, page: page, pages: pages };
}

/** A line of text on a page: its face, size, baseline (points from the top left), anchor and color (r g b, 0..1). */
/** type PageText = { face: String, size: Number, x: Number, y: Number, text: String, anchor: String, color: String } */

const SOFT = "0.35 0.31 0.26";
/** The quarter note of the tempo mark, in points per staff space of its glyph. */
const MET = 2.6;

/** The words around the music on page `p`: the title block on the first, a page number on the others. */
/** function pageTexts(lay: PdfLayout, p: Int, info: PdfInfo) => PageText[] */
function pageTexts(lay, p, info) {
  const W = lay.w;
  const H = lay.h;
  /** const out: PageText[] */
  const out = [];
  if (p > 0) {
    out.push({ face: "Times-Roman", size: 9, x: W / 2, y: H - lay.bottom * 0.5, text: String(p + 1), anchor: "middle", color: SOFT });
    return out;
  }
  out.push({ face: "Times-Bold", size: 21, x: W / 2, y: lay.top + 8 * MM, text: info.title, anchor: "middle", color: INK });
  if (info.subtitle !== "") out.push({ face: "Times-Italic", size: 10.5, x: W / 2, y: lay.top + 14 * MM, text: info.subtitle, anchor: "middle", color: SOFT });
  const row = lay.top + 22 * MM;
  if (info.bpm > 0)
    out.push({
      face: "Times-Bold",
      size: 10,
      x: lay.left + G.metNoteQuarterUp.x1 * MET + 2,
      y: row,
      text: `= ${Math.round(info.bpm)}`,
      anchor: "start",
      color: INK,
    });
  if (info.author !== "") out.push({ face: "Times-Italic", size: 10, x: W - lay.left, y: row, text: info.author, anchor: "end", color: SOFT });
  out.push({ face: "Times-Italic", size: 7, x: W / 2, y: H - lay.bottom * 0.45, text: "Engraved with Rosaclef", anchor: "middle", color: "0.55 0.5 0.44" });
  return out;
}

/** Face and size of a label in a system (staff spaces). */
/** function labelFace(cls: String) => String */
function labelFace(cls) {
  return cls === "volta" ? "Times-Bold" : cls === "reptimes" ? "Times-BoldItalic" : "Times-Italic";
}

/** function labelSize(cls: String) => Number */
function labelSize(cls) {
  return cls === "mnum" ? 1.25 : cls === "sname short" ? 1.3 : cls === "volta" ? 1.45 : 1.55;
}

/** The color of a run of ink: plain, the staff's, or a passage's (deepened, as printed). */
/** function inkRgb(color: String) => String */
function inkRgb(color) {
  return color === "" ? INK : color === "staff" ? STAFF : rgb(color, 0.68);
}

/** Lay a score out on pages and write the PDF objects (catalog first, document info last). */
/** function scorePdf(sc: Score, info: PdfInfo, paper: String, hideEmpty: Boolean, m: Measurer) => PdfObj[] */
export function scorePdf(sc, info, paper, hideEmpty, m) {
  const lay = pdfLayout(sc, paper, hideEmpty);
  const W = lay.w;
  const H = lay.h;
  const left = lay.left;
  const sp = lay.sp;
  const page = lay.page;
  const pages = lay.pages;

  // Objects: 1 catalog, 2 pages, 3–6 fonts, 7 the bands' transparency, then glyph forms, resources, page contents, info.
  /** const objs: PdfObj[] */
  const objs = [
    { head: "<< /Type /Catalog /Pages 2 0 R >>", stream: "" },
    { head: "", stream: "" },
  ];
  for (const f of FONTS) objs.push({ head: `<< /Type /Font /Subtype /Type1 /BaseFont /${f} /Encoding /WinAnsiEncoding >>`, stream: "" });
  objs.push({ head: "<< /Type /ExtGState /ca 0.16 >>", stream: "" });
  const firstGlyph = objs.length + 1;
  for (const g of GLYPHS) {
    objs.push({
      head: `<< /Type /XObject /Subtype /Form /BBox [${n(g.x0 - 0.2)} ${n(g.top - 0.2)} ${n(g.x1 + 0.2)} ${n(g.bottom + 0.2)}] >>`,
      stream: `${pathOps(g.d)}\nf`,
    });
  }
  /** const xo: String[] */
  const xo = [];
  for (let i = 0; i < GLYPHS.length; i++) xo.push(`/G${i} ${firstGlyph + i} 0 R`);
  const resources = objs.length + 1;
  objs.push({
    head: `<< /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R /F4 6 0 R >> /ExtGState << /GSb 7 0 R >> /XObject << ${xo.join(" ")} >> >>`,
    stream: "",
  });

  /** function glyphName(ch: String) => String */
  function glyphName(ch) {
    for (let i = 0; i < GLYPHS.length; i++) if (GLYPHS[i].c === ch) return `/G${i}`;
    return "";
  }

  /** const kids: String[] */
  const kids = [];
  for (let p = 0; p < pages.length; p++) {
    /** const ops: String[] */
    const ops = [];
    for (const t of pageTexts(lay, p, info)) ops.push(textAt(m, t.face, t.size, t.x, H - t.y, t.text, t.anchor, t.color));
    // The tempo's quarter note, scaled to its text.
    if (p === 0 && info.bpm > 0)
      ops.push(`${INK} rg q ${n(MET)} 0 0 ${n(-MET)} ${n(left)} ${n(H - lay.top - 22 * MM)} cm ${glyphName(G.metNoteQuarterUp.c)} Do Q`);

    for (const pl of pages[p]) {
      const s = page.systems[pl.sys];
      // System coordinates are staff spaces, y down: one transform maps them onto the page.
      ops.push(`q ${n(sp)} 0 0 ${n(-sp)} ${n(left)} ${n(H - pl.top)} cm`);
      for (const b of s.bands) ops.push(`q /GSb gs ${rgb(b.color, 1)} rg ${n(b.x)} ${n(b.y)} ${n(b.w)} ${n(b.h)} re f Q`);
      for (const ink of s.inks) {
        // The wet ink's gloss is for the screen; print is dry.
        if (ink.color === GLOSS || ink.color === SHEEN) continue;
        const color = inkRgb(ink.color);
        if (ink.d !== "") ops.push(`${color} rg\n${pathOps(ink.d)}\nf`);
        if (ink.text === "") continue;
        const xs = ink.xs.split(" ");
        const ys = ink.ys.split(" ");
        const chars = ink.text.split("");
        // Glyphs are single UTF-16 units (the font's private use area).
        for (let k = 0; k < chars.length && k < xs.length; k++) {
          const name = glyphName(chars[k]);
          if (name !== "") ops.push(`${color} rg q 1 0 0 1 ${xs[k]} ${ys[k]} cm ${name} Do Q`);
        }
      }
      for (const br of s.braces) ops.push(`${INK} rg q ${n(br.s)} 0 0 ${n(br.s)} ${n(br.x)} ${n(br.y)} cm ${glyphName(G.brace.c)} Do Q`);
      for (const l of s.labels) {
        const face = labelFace(l.cls);
        const size = labelSize(l.cls);
        const w = m(face, shown(l.text)) * size;
        const x0 = l.anchor === "end" ? l.x - w : l.anchor === "middle" ? l.x - w / 2 : l.x;
        // Text is drawn upright again inside the flipped system.
        ops.push(`${l.cls === "mnum" ? SOFT : INK} rg BT ${fontRef(face)} ${n(size)} Tf 1 0 0 -1 ${n(x0)} ${n(l.y)} Tm ${pdfString(l.text)} Tj ET`);
      }
      for (const b of s.bands) {
        if (b.label === "" || !b.first) continue;
        ops.push(`${rgb(b.color, 0.68)} rg BT /F4 1.35 Tf 1 0 0 -1 ${n(b.x + 0.5)} ${n(b.y - 0.45)} Tm ${pdfString(b.label)} Tj ET`);
      }
      ops.push("Q");
    }
    const contents = objs.length + 1;
    objs.push({ head: "<< >>", stream: ops.join("\n") });
    objs.push({ head: `<< /Type /Page /Parent 2 0 R /MediaBox [0 0 ${n(W)} ${n(H)}] /Resources ${resources} 0 R /Contents ${contents} 0 R >>`, stream: "" });
    kids.push(`${objs.length} 0 R`);
  }
  objs[1] = { head: `<< /Type /Pages /Kids [${kids.join(" ")}] /Count ${kids.length} >>`, stream: "" };
  objs.push({ head: `<< /Title ${pdfString(info.title)} /Creator (Rosaclef Studio) /Producer (Rosaclef) >>`, stream: "" });
  return objs;
}

// ------------------------------------------------------------------ as on screen

/** The look of a page drawn as on screen: wet or dry ink, and the wet ink's knobs (see ink.js). */
/** type PageLook = { wet: Boolean, gloss: Number, shine: Number } */

/** "r g b" (0..1) as a CSS color. */
/** function css(c: String) => String */
function css(c) {
  const v = c.split(" ").map((x) => Math.round(Number(x) * 255));
  return `rgb(${v[0]},${v[1]},${v[2]})`;
}

/** Text as XML character data. */
/** function esc(s: String) => String */
function esc(s) {
  return s.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");
}

const SERIF = "'Times New Roman', 'Liberation Serif', Tinos, Times, serif";

/** SVG text attributes for a standard face. */
/** function faceAttrs(face: String, size: Number) => String */
function faceAttrs(face, size) {
  const italic = face.includes("Italic") ? ` font-style="italic"` : "";
  const bold = face.includes("Bold") ? ` font-weight="bold"` : "";
  return `font-family="${SERIF}" font-size="${n(size)}"${italic}${bold}`;
}

/**
 * Page `p` as an SVG document of `w`·`scale` by `h`·`scale` pixels (its units
 * are points): the paper with its textures and toned edges, and the music
 * drawn as on screen — through the ink filter, with the wet ink's glints.
 */
/** function pageSvg(lay: PdfLayout, p: Int, info: PdfInfo, look: PageLook, scale: Number) => String */
export function pageSvg(lay, p, info, look, scale) {
  return pagePart(lay, p, info, look, scale, [0, 0, lay.w, lay.h]);
}

/** The band of its page a system takes (with room for what sticks out): [x, y, w, h] in points. */
/** function stripBox(lay: PdfLayout, sys: Int) => Number[] */
export function stripBox(lay, sys) {
  let top = 0;
  for (const list of lay.pages) for (const on of list) if (on.sys === sys) top = on.top;
  const s = lay.page.systems[sys];
  const y0 = Math.max(0, top - 3.5 * lay.sp);
  const y1 = Math.min(lay.h, top + (s.height + 3.5) * lay.sp);
  return [0, y0, lay.w, y1 - y0];
}

/**
 * Part of page `p` as SVG: the points of `box` ([x, y, w, h]) drawn at `scale`
 * pixels a point — the whole page (pageSvg), or one system's band of it, sharp
 * enough to look at closely (the film's close-ups).
 */
/** function pagePart(lay: PdfLayout, p: Int, info: PdfInfo, look: PageLook, scale: Number, box: Number[]) => String */
export function pagePart(lay, p, info, look, scale, box) {
  return drawPart(lay, p, info, look, scale, box, false);
}

/**
 * The ink of part of a page, as a height map: black where ink lies (a little
 * softened, so it rounds off like a raised line), white paper. The film
 * raises the ink off the paper with it and lights it.
 */
/** function inkPart(lay: PdfLayout, p: Int, info: PdfInfo, scale: Number, box: Number[]) => String */
export function inkPart(lay, p, info, scale, box) {
  return drawPart(lay, p, info, { wet: false, gloss: 0, shine: 0 }, scale, box, true);
}

/** function drawPart(lay: PdfLayout, p: Int, info: PdfInfo, look: PageLook, scale: Number, box: Number[], mask: Boolean) => String */
function drawPart(lay, p, info, look, scale, box, mask) {
  /** The color of a mark (all black for the height map). */
  /** function fillOf(c: String) => String */
  function fillOf(c) {
    return mask ? "#000" : css(c);
  }
  const W = lay.w;
  const H = lay.h;
  const sp = lay.sp;
  /** const used: Int[] */
  const used = [];
  /** A placed glyph, defined once below. */
  /** function use(ch: String, x: String, y: String) => String */
  function use(ch, x, y) {
    const i = GLYPHS.findIndex((g) => g.c === ch);
    if (i < 0) return "";
    if (!used.includes(i)) used.push(i);
    return `<use href="#G${i}" x="${x}" y="${y}"/>`;
  }
  /** const body: String[] */
  const body = [];
  for (const t of pageTexts(lay, p, info)) {
    body.push(`<text x="${n(t.x)}" y="${n(t.y)}" text-anchor="${t.anchor}" fill="${fillOf(t.color)}" ${faceAttrs(t.face, t.size)}>${esc(t.text)}</text>`);
  }
  if (p === 0 && info.bpm > 0) {
    body.push(
      `<g transform="translate(${n(lay.left)} ${n(lay.top + 22 * MM)}) scale(${MET})" fill="${fillOf(INK)}">${use(G.metNoteQuarterUp.c, "0", "0")}</g>`
    );
  }
  for (const pl of lay.pages[p]) {
    const s = lay.page.systems[pl.sys];
    // Systems outside the box are left out (and their filters not run).
    if (pl.top > box[1] + box[3] || pl.top + s.height * sp < box[1]) continue;
    // System coordinates are staff spaces, y down, like the page's.
    body.push(`<g transform="translate(${n(lay.left)} ${n(pl.top)}) scale(${n(sp)})">`);
    if (!mask)
      for (const b of s.bands)
        body.push(`<rect x="${n(b.x)}" y="${n(b.y)}" width="${n(b.w)}" height="${n(b.h)}" rx="0.8" fill="${css(rgb(b.color, 1))}" fill-opacity="0.16"/>`);
    // Staff lines are hairlines: drawn plainly, as on screen.
    for (const ink of s.inks) if (ink.color === "staff" && ink.d !== "") body.push(`<path d="${ink.d}" fill="${fillOf(STAFF)}"/>`);
    body.push(mask ? `<g filter="url(#soft)">` : `<g filter="url(#ink)">`);
    for (const ink of s.inks) {
      if (ink.color === GLOSS || ink.color === SHEEN || ink.color === "staff") continue;
      const fill = fillOf(inkRgb(ink.color));
      if (ink.d !== "") body.push(`<path d="${ink.d}" fill="${fill}"/>`);
      if (ink.text === "") continue;
      const xs = ink.xs.split(" ");
      const ys = ink.ys.split(" ");
      const chars = ink.text.split("");
      body.push(`<g fill="${fill}">`);
      for (let k = 0; k < chars.length && k < xs.length; k++) body.push(use(chars[k], xs[k], ys[k]));
      body.push("</g>");
    }
    for (const br of s.braces)
      body.push(`<g transform="translate(${n(br.x)} ${n(br.y)}) scale(${n(br.s)})" fill="${fillOf(INK)}">${use(G.brace.c, "0", "0")}</g>`);
    for (const l of s.labels) {
      const color = l.cls === "mnum" ? SOFT : INK;
      body.push(
        `<text x="${n(l.x)}" y="${n(l.y)}" text-anchor="${l.anchor}" fill="${fillOf(color)}" ${faceAttrs(labelFace(l.cls), labelSize(l.cls))}>${esc(l.text)}</text>`
      );
    }
    for (const b of s.bands) {
      if (b.label === "" || !b.first || mask) continue;
      body.push(
        `<text x="${n(b.x + 0.5)}" y="${n(b.y - 0.45)}" fill="${css(rgb(b.color, 0.68))}" ${faceAttrs("Times-BoldItalic", 1.35)}>${esc(b.label)}</text>`
      );
    }
    body.push("</g>");
    // The wet ink's glints, in light over the music.
    if (look.wet && !mask) {
      for (const run of [SHEEN, GLOSS]) {
        const ink = s.inks.find((x) => x.color === run);
        if (ink) body.push(`<path d="${ink.d}" fill="#fffcf2" fill-opacity="${n(0.9 * glintOpacity(run, look.gloss, look.shine))}" filter="url(#${run})"/>`);
      }
    }
    body.push("</g>");
  }

  // The paper: its tiles scaled with the music, as they are on screen at 100% (a staff space of 7 pixels).
  const k = sp / 7;
  /** function tile(id: String, t: Tile) => String */
  function tile(id, t) {
    const size = n(t.size * k);
    return `<pattern id="${id}" patternUnits="userSpaceOnUse" width="${size}" height="${size}"><image href="${t.url}" width="${size}" height="${size}"/></pattern>`;
  }
  const defs = [
    markup(inkFilter("ink", look.wet, false, look.gloss, look.shine, sp * scale)),
    `<filter id="sheen"><feGaussianBlur stdDeviation="0.09"/></filter>`,
    `<filter id="gloss"><feGaussianBlur stdDeviation="0.025"/></filter>`,
    `<filter id="edge" x="-10%" y="-10%" width="120%" height="120%"><feGaussianBlur stdDeviation="${n(3 * sp)}"/></filter>`,
    tile("tooth", PAPER.tooth),
    tile("mottle", PAPER.mottle),
    tile("grain", PAPER.grain),
  ];
  for (const i of used) defs.push(`<path id="G${i}" d="${GLYPHS[i].d}"/>`);
  if (mask) {
    // The height map: white paper, the ink softened by about a tenth of a staff space.
    const glyphs = defs.filter((d) => d.startsWith("<path"));
    return (
      `<svg xmlns="http://www.w3.org/2000/svg" width="${Math.round(box[2] * scale)}" height="${Math.round(box[3] * scale)}" viewBox="${n(box[0])} ${n(box[1])} ${n(box[2])} ${n(box[3])}">` +
      `<defs><filter id="soft" x="-5%" y="-5%" width="110%" height="110%"><feGaussianBlur stdDeviation="0.16"/></filter>${glyphs.join("")}</defs><rect width="${n(W)}" height="${n(H)}" fill="#fff"/>${body.join("")}</svg>`
    );
  }
  const paper = [
    `<rect width="${n(W)}" height="${n(H)}" fill="${PAPER.color}"/>`,
    `<rect width="${n(W)}" height="${n(H)}" fill="url(#tooth)"/>`,
    `<rect width="${n(W)}" height="${n(H)}" fill="url(#mottle)"/>`,
    `<rect width="${n(W)}" height="${n(H)}" fill="url(#grain)"/>`,
    // Edges warmed a little, as paper tones with age.
    `<rect width="${n(W)}" height="${n(H)}" fill="none" stroke="rgb(150,106,38)" stroke-opacity="0.22" stroke-width="${n(6 * sp)}" filter="url(#edge)"/>`,
  ];
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${Math.round(box[2] * scale)}" height="${Math.round(box[3] * scale)}" viewBox="${n(box[0])} ${n(box[1])} ${n(box[2])} ${n(box[3])}">` +
    `<defs>${defs.join("")}</defs>${paper.join("")}${body.join("")}</svg>`
  );
}

/** The document info of a PDF titled `title`. */
/** function pdfInfo(title: String) => String */
export function pdfInfo(title) {
  return `<< /Title ${pdfString(title)} /Creator (Rosaclef Studio) /Producer (Rosaclef) >>`;
}
