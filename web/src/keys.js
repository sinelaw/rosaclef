// Global keyboard shortcuts (FL Studio conventions where they exist) and the
// computer-keyboard piano (its key map lives in ui/keyboard.js).
//
// The piano takes two whole letter rows, so letter shortcuts take Shift:
// the one modifier that means the same on every platform and that no browser
// or OS claims for itself (Ctrl/Cmd+Q, R, P, E and L quit, reload, print or
// grab the address bar; Alt opens menus on Windows and types accents on a Mac).

import { listenWindow } from "#platform";
import { state, undo, redo, currentChannel } from "./store.js";
import { togglePlay, stop, setMode, record, toggleMetronome } from "./audio.js";
import { deleteSelection, selectAll, transpose, quantize, duplicateSelection, setTool } from "./ui/pianoroll.js";
import { deleteSelectedClips } from "./ui/playlist.js";
import { auto, closeMenu } from "./automation.js";
import { openDock, paneShortcut } from "./ui/panes.js";
import { keyboard, pressKey, releaseKey, typedPitch, shiftTyped, toggleRecordKeys } from "./ui/keyboard.js";
import { voice, startTake, stopTake } from "./ui/voice.js";
import { setScoreTool, cancelScoreRange } from "./ui/score.js";

/** Computer keys holding a note, by `code`. */
/** const held: String[] */
const held = [];

/** Shift+<letter> shortcuts; `c` is the lower-case letter. Returns true when handled. */
/** function letterShortcut(c: String) => Boolean */
function letterShortcut(c) {
  if (c === "l") setMode(state.mode === "pattern" ? "song" : "pattern");
  // In the Voice dock, Shift+R records a take to turn into notes.
  else if (c === "r" && state.dock === "voice") startTake();
  else if (c === "r") record();
  else if (c === "m") toggleMetronome();
  else if (c === "q" && state.dock === "piano") quantize();
  else if (c === "p" && state.focus === "score") setScoreTool("write");
  else if (c === "e" && state.focus === "score") setScoreTool("select");
  else if (c === "h" && state.focus === "score") setScoreTool("pan");
  else if (c === "p" && state.dock === "piano") setTool("draw");
  else if (c === "e" && state.dock === "piano") setTool("select");
  else return false;
  return true;
}

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
    if (e.shiftKey && !e.altKey && !e.repeat && letterShortcut(k.toLowerCase())) {
      e.preventDefault();
      return undefined;
    }
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
      else if (cancelScoreRange()) return undefined;
      else if (voice.status === "recording") stopTake();
      else if (keyboard.armed) toggleRecordKeys();
      else stop();
    } else if (k === "F4") {
      e.preventDefault();
      openDock("drums");
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
    } else if (k === "F10") {
      e.preventDefault();
      openDock("score");
    } else if (k === "Delete" || k === "Backspace") {
      if ((state.dock === "piano" || state.focus === "score") && state.selection.length > 0) deleteSelection();
      else deleteSelectedClips();
    } else if ((state.dock === "piano" || state.focus === "score") && (k === "ArrowUp" || k === "ArrowDown")) {
      e.preventDefault();
      transpose((k === "ArrowUp" ? 1 : -1) * (e.shiftKey ? 12 : 1));
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
