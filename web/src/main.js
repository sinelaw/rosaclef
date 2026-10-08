// Rosaclef Studio — entry point.

import { domBackend, getJson, listenWindow } from "#platform";
import { state, hooks, invalidate } from "./store.js";
import { decodeCatalog } from "./model.js";
import { mount } from "../tree/tree.js";
import { studio } from "./ui/shell.js";
import { connect, installSync } from "./net.js";
import { installEngine, startAudio, loadMetronome } from "./audio.js";
import { installKeys } from "./keys.js";
import { loadAgents } from "./ui/agent.js";
import { loadLayout } from "./ui/panes.js";
import { loadKeyboard } from "./ui/keyboard.js";
import { loadScorePrefs } from "./ui/score.js";
import { loadLanguage } from "./i18n.js";

loadLayout();
loadKeyboard();
loadMetronome();
loadScorePrefs();
// The studio shows once its texts are in (English, and the language chosen).
const boot = { started: false };
loadLanguage(() => {
  if (boot.started) invalidate();
  else start();
});

function start() {
  boot.started = true;
  const ui = mount(domBackend("app"), studio);
  hooks.mark = ui.mark;

  installSync();
  installEngine();
  installKeys();
  connect();
  loadAgents();

  getJson("/api/catalog")
    .then((c) => {
      state.catalog = decodeCatalog(c);
      invalidate();
      return true;
    })
    .catch((e) => false);

  // Browsers only start audio after a user gesture.
  listenWindow("pointerdown", (e) => {
    if (!state.audioReady && state.output === "browser") startAudio();
  });
  listenWindow("resize", (e) => {
    invalidate();
  });

  ui.flush();
}
