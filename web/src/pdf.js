// Sheet music as a PDF: the score engraved again for a printed page (A4 or
// US Letter), paginated, and written as PDF objects.
//
// Everything is vector. The music glyphs are drawn from Bravura's outlines
// (`d` in smufl.js), each defined once as a form and placed wherever it
// appears, so no font is embedded and every viewer shows the same shapes.
// Text uses the PDF standard Times faces. The platform layer compresses
// the streams and writes the file (downloadPdf in web/lib/platform.js).

import { G, GLYPHS } from "./smufl.js";
import { engrave, GLOSS } from "./engrave.js";

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

/** Lay a score out on pages and write the PDF objects (catalog first, document info last). */
/** function scorePdf(sc: Score, info: PdfInfo, paper: String, hideEmpty: Boolean, m: Measurer) => PdfObj[] */
export function scorePdf(sc, info, paper, hideEmpty, m) {
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
    if (p === 0) {
      ops.push(textAt(m, "Times-Bold", 21, W / 2, H - top - 8 * MM, info.title, "middle", INK));
      if (info.subtitle !== "") ops.push(textAt(m, "Times-Italic", 10.5, W / 2, H - top - 14 * MM, info.subtitle, "middle", "0.35 0.31 0.26"));
      const row = H - top - 22 * MM;
      if (info.bpm > 0) {
        // A quarter note (scaled to the text) and the tempo.
        const s = 2.6;
        ops.push(`${INK} rg q ${n(s)} 0 0 ${n(-s)} ${n(left)} ${n(row)} cm ${glyphName(G.metNoteQuarterUp.c)} Do Q`);
        ops.push(textAt(m, "Times-Bold", 10, left + G.metNoteQuarterUp.x1 * s + 2, row, `= ${Math.round(info.bpm)}`, "start", INK));
      }
      if (info.author !== "") ops.push(textAt(m, "Times-Italic", 10, W - left, row, info.author, "end", "0.35 0.31 0.26"));
      ops.push(textAt(m, "Times-Italic", 7, W / 2, bottom * 0.45, "Engraved with Rosaclef", "middle", "0.55 0.5 0.44"));
    } else ops.push(textAt(m, "Times-Roman", 9, W / 2, bottom * 0.5, String(p + 1), "middle", "0.35 0.31 0.26"));

    for (const pl of pages[p]) {
      const s = page.systems[pl.sys];
      // System coordinates are staff spaces, y down: one transform maps them onto the page.
      ops.push(`q ${n(sp)} 0 0 ${n(-sp)} ${n(left)} ${n(H - pl.top)} cm`);
      for (const b of s.bands) ops.push(`q /GSb gs ${rgb(b.color, 1)} rg ${n(b.x)} ${n(b.y)} ${n(b.w)} ${n(b.h)} re f Q`);
      for (const ink of s.inks) {
        // The wet ink's gloss is for the screen; print is dry.
        if (ink.color === GLOSS) continue;
        const color = ink.color === "" ? INK : ink.color === "staff" ? STAFF : rgb(ink.color, 0.68);
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
        const face = "Times-Italic";
        const size = l.cls === "mnum" ? 1.25 : l.cls === "sname short" ? 1.3 : 1.55;
        const w = m(face, shown(l.text)) * size;
        const x0 = l.anchor === "end" ? l.x - w : l.anchor === "middle" ? l.x - w / 2 : l.x;
        // Text is drawn upright again inside the flipped system.
        ops.push(`${l.cls === "mnum" ? "0.35 0.31 0.26" : INK} rg BT ${fontRef(face)} ${n(size)} Tf 1 0 0 -1 ${n(x0)} ${n(l.y)} Tm ${pdfString(l.text)} Tj ET`);
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
