// Global keyboard shortcuts (FL Studio conventions where they exist) and the
// computer-keyboard piano.

import { listenWindow } from "#platform";
import { state, undo, redo, showDock, currentChannel } from "./store.js";
import { togglePlay, stop, setMode, record, noteOn, noteOff } from "./audio.js";
import { deleteSelection, selectAll, transpose, quantize, duplicateSelection, setTool } from "./ui/pianoroll.js";
import { deleteSelectedClips } from "./ui/playlist.js";
import { auto, closeMenu } from "./automation.js";

// Lower keyboard row plays C4..C5 on the selected channel.
const PIANO = ["z", "s", "x", "d", "c", "v", "g", "b", "h", "n", "j", "m", ","];

/** const held: String[] */
const held = [];

export function installKeys() {
  listenWindow("keydown", (e) => {
    if (e.typing) return undefined;
    const k = e.key;
    const mod = e.ctrlKey || e.metaKey;
    if (mod && (k === "z" || k === "Z")) {
      e.preventDefault();
      if (e.shiftKey) redo();
      else undo();
      return undefined;
    }
    if (mod && (k === "y" || k === "Y")) {
      e.preventDefault();
      redo();
      return undefined;
    }
    if (mod && (k === "a" || k === "A") && state.dock === "piano") {
      e.preventDefault();
      selectAll();
      return undefined;
    }
    if (mod && (k === "d" || k === "D") && state.dock === "piano") {
      e.preventDefault();
      duplicateSelection();
      return undefined;
    }
    if (mod) return undefined;
    if (k === " ") {
      e.preventDefault();
      togglePlay();
    } else if (k === "Escape") {
      if (auto.menu.open) closeMenu();
      else stop();
    } else if (k === "F6") {
      e.preventDefault();
      showDock("rack");
    } else if (k === "F7") {
      e.preventDefault();
      showDock("piano");
    } else if (k === "F9") {
      e.preventDefault();
      showDock("mixer");
    } else if (k === "l" || k === "L") {
      setMode(state.mode === "pattern" ? "song" : "pattern");
    } else if (k === "r" || k === "R") {
      record();
    } else if (k === "Delete" || k === "Backspace") {
      if (state.dock === "piano" && state.selection.length > 0) deleteSelection();
      else deleteSelectedClips();
    } else if (state.dock === "piano" && (k === "ArrowUp" || k === "ArrowDown")) {
      e.preventDefault();
      transpose((k === "ArrowUp" ? 1 : -1) * (e.shiftKey ? 12 : 1));
    } else if (state.dock === "piano" && (k === "q" || k === "Q")) {
      quantize();
    } else if (state.dock === "piano" && (k === "p" || k === "P")) {
      setTool("draw");
    } else if (state.dock === "piano" && (k === "e" || k === "E")) {
      setTool("select");
    } else {
      const i = PIANO.indexOf(k);
      const ch = currentChannel();
      if (i >= 0 && ch && !e.repeat && !held.includes(k)) {
        held.push(k);
        noteOn(ch.id, 60 + i, 0.85);
      }
    }
    return undefined;
  });
  listenWindow("keyup", (e) => {
    const k = e.key;
    const at = held.indexOf(k);
    if (at >= 0) {
      held.splice(at, 1);
      const ch = currentChannel();
      if (ch) noteOff(ch.id, 60 + PIANO.indexOf(k));
    }
    return undefined;
  });
}
