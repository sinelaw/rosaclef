// Global keyboard shortcuts (FL Studio conventions where they exist) and the
// computer-keyboard piano (its key map lives in ui/keyboard.js).

import { listenWindow } from "#platform";
import { state, undo, redo, currentChannel } from "./store.js";
import { togglePlay, stop, setMode, record } from "./audio.js";
import { deleteSelection, selectAll, transpose, quantize, duplicateSelection, setTool } from "./ui/pianoroll.js";
import { deleteSelectedClips } from "./ui/playlist.js";
import { auto, closeMenu } from "./automation.js";
import { openDock, paneShortcut } from "./ui/panes.js";
import { keyboard, pressKey, releaseKey, typedPitch, shiftTyped, toggleRecordKeys } from "./ui/keyboard.js";
import { voice, startTake, stopTake } from "./ui/voice.js";

/** Computer keys holding a note, by `code`. */
/** const held: String[] */
const held = [];

export function installKeys() {
  listenWindow("keydown", (e) => {
    if (e.typing) return undefined;
    const k = e.key;
    const mod = e.ctrlKey || e.metaKey;
    // Ctrl+Alt+B/A/P/D/0: minimize, maximize and restore the panels.
    if (mod && e.altKey && paneShortcut(e.code)) {
      e.preventDefault();
      return undefined;
    }
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
    // The piano first: with the typing keyboard on, its keys win over the
    // letter shortcuts they share (off, Z–M and "," share none).
    const pitch = typedPitch(e.code);
    if (pitch >= 0) {
      if (currentChannel() && !e.repeat && !held.includes(e.code)) {
        held.push(e.code);
        pressKey(`k${e.code}`, pitch, 0.85);
      }
      return undefined;
    }
    if (e.code === "Minus" || e.code === "Equal") {
      shiftTyped(e.code === "Minus" ? -1 : 1);
      return undefined;
    }
    if (k === " ") {
      e.preventDefault();
      togglePlay();
    } else if (k === "Escape") {
      if (auto.menu.open) closeMenu();
      else if (voice.status === "recording") stopTake();
      else if (keyboard.armed) toggleRecordKeys();
      else stop();
    } else if (k === "F6") {
      e.preventDefault();
      openDock("rack");
    } else if (k === "F7") {
      e.preventDefault();
      openDock("piano");
    } else if (k === "F8") {
      e.preventDefault();
      openDock("voice");
    } else if (k === "F9") {
      e.preventDefault();
      openDock("mixer");
    } else if (k === "l" || k === "L") {
      setMode(state.mode === "pattern" ? "song" : "pattern");
    } else if (k === "r" || k === "R") {
      // In the Voice dock, R records a take to turn into notes.
      if (state.dock === "voice") startTake();
      else record();
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
    }
  });
  listenWindow("keyup", (e) => {
    const at = held.indexOf(e.code);
    if (at >= 0) {
      held.splice(at, 1);
      releaseKey(`k${e.code}`);
    }
  });
}
