// Small projects for the lyrics and reuse tests: a lead and a drum channel,
// patterns written as [channel (0 lead, 1 kit), pitch, start] rows.
import { tokenText } from "../src/lyrics.js";
import { emptyProject, noArp } from "../src/model.js";
import { insertIx } from "#brands";

/** A project with a lead and a drum channel. */
/** function project() => Project */
export function project() {
  const p = emptyProject();
  p.patterns = [];
  for (const [id, type] of [
    ["lead", "synth"],
    ["kit", "drum"],
  ]) {
    p.channels.push({
      id: id,
      name: id,
      color: "#c9a45c",
      instrument: { type: type, enabled: true, params: [], options: [] },
      volume: 0.8,
      pan: 0,
      mute: false,
      mixer: insertIx(0),
      arp: noArp(),
    });
  }
  return p;
}

/** function pat(id: String, length: Number, notes: Number[][]) => Pattern */
export function pat(id, length, notes) {
  return {
    id: id,
    name: id,
    color: "#c9a45c",
    length: length,
    notes: notes.map((n) => ({ channel: n[0] === 0 ? "lead" : "kit", pitch: n[1], start: n[2], length: 1, velocity: 0.8 })),
    uses: [],
    lyrics: [],
  };
}

/** function use(pattern: String, start: Number) => Use */
export function use(pattern, start) {
  return { pattern: pattern, start: start, from: 0, to: -1, transpose: 0, channel: "", velocity: 1, verse: 0 };
}

/** function word(s: Sung | Undefined) => String */
function word(s) {
  return s === undefined ? "." : tokenText(s.token);
}

/** function words(notes: Sounding[]) => String */
export function words(notes) {
  return notes.map((n) => word(n.sung)).join(" ");
}

/** A singing line for the lead: verses in order, from verse 1. */
/** function line(verses: String[]) => Lyrics */
export function line(verses) {
  /** const kv: KS[] */
  const kv = [];
  for (let i = 0; i < verses.length; i++) kv.push({ key: String(i + 1), value: verses[i] });
  return { channel: "lead", lang: "", mode: "sing", verses: kv, timing: [] };
}
