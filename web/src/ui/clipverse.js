// The verse a pattern clip sings (Clip.verse) in the playlist: a "v2" badge
// on the clip, and a Verse choice in the playlist's tools for the selected
// clips whose patterns have words. Auto (0) sings the pass of the repeat
// the clip plays in, else the first verse.

import { state, commit } from "../store.js";
import { patternIndex, verseCount, sings } from "../expand.js";
import { select } from "./widgets.js";
import { clipIndex } from "#brands";

/** The selected clips that play a pattern with words. */
/** function singingClips() => Clip[] */
function singingClips() {
  const p = state.project;
  /** const out: Clip[] */
  const out = [];
  for (const s of state.clipSelection) {
    const c = p.playlist.clips[clipIndex(s)];
    if (c === undefined || c.pattern === "") continue;
    const j = patternIndex(p, c.pattern);
    if (j >= 0 && sings(p, j)) out.push(c);
  }
  return out;
}

/** The badge of a clip that sings a chosen verse. */
/** function verseBadge(b: Builder, c: Clip) => Undefined */
export function verseBadge(b, c) {
  if (c.verse <= 0) return undefined;
  b.leaf("span", "verse", "clip-verse", `v${c.verse}`);
  b.attr("title", `Sings verse ${c.verse}`);
}

/** Set the verse of the selected clips that sing (one undo step). */
/** function setVerses(verse: Int) => Undefined */
function setVerses(verse) {
  const clips = singingClips();
  if (clips.length > 0) {
    commit(() => {
      for (const c of clips) c.verse = verse;
    });
  }
}

/** The Verse choice, while the selection holds clips that sing. */
/** function verseTool(b: Builder) => Undefined */
export function verseTool(b) {
  const clips = singingClips();
  if (clips.length === 0) return undefined;
  const p = state.project;
  let most = 1;
  for (const c of clips) most = Math.max(most, Math.max(c.verse, verseCount(p, patternIndex(p, c.pattern))));
  const choices = ["0"];
  const labels = ["Auto"];
  for (let v = 1; v <= most; v++) {
    choices.push(String(v));
    labels.push(`Verse ${v}`);
  }
  b.leaf("span", "vl", "label", "Sings");
  select(
    b,
    "verse",
    "",
    String(clips[0].verse),
    choices,
    labels,
    "The verse the selected clips sing (Auto: the pass of the repeat they play in, else the first)",
    (v) => setVerses(Math.round(Number(v)))
  );
}
