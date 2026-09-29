// Rosaclef Studio — entry point.

import { domBackend, getJson, listenWindow } from "#platform";
import { state, hooks, invalidate } from "./store.js";
import { mount } from "./ui/tree.js";
import { studio } from "./ui/shell.js";
import { connect, installSync } from "./net.js";
import { installEngine, startAudio } from "./audio.js";
import { installKeys } from "./keys.js";
import { loadAgents } from "./ui/agent.js";

const ui = mount(domBackend("app"), studio);
hooks.mark = ui.mark;

installSync();
installEngine();
installKeys();
connect();
loadAgents();

getJson("/api/catalog")
  .then((c) => {
    state.catalog = c;
    invalidate();
    return Promise.resolve(true);
  })
  .catch((e) => Promise.resolve(false));

// Browsers only start audio after a user gesture.
listenWindow("pointerdown", (e) => {
  if (!state.audioReady && state.output === "browser") startAudio();
  return undefined;
});
listenWindow("resize", (e) => {
  invalidate();
  return undefined;
});

ui.flush();
