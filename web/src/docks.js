// The dock's tabs, in one table: the shell's tab strip and the phone's
// navigation bar show them in this order, the function keys open them, and the
// agent's context names them. (Each tab's body and tools are bound to its id in
// ui/shell.js: this module imports no panel, so the store and the network layer
// can read it.)

import { tk } from "./i18n.js";

/** A dock tab. `label`, `navLabel`: message keys of its tab and of its button
 * in the phone's navigation bar. `key`: the function key that opens it.
 * `agentName`: how the agent's context and the panel focus name it. `keys`:
 * whether the on-screen piano shows under it on a phone: not in the mixer,
 * nor while singing into the microphone (the phone frees the room). */
/** type DockInfo = { id: String, label: String, navLabel: String, icon: String, key: String, agentName: String, keys: Boolean } */

/** const DOCKS: DockInfo[] */
export const DOCKS = [
  {
    id: "rack",
    label: tk("shell.dock.rack.label"),
    navLabel: tk("shell.nav.rack.label"),
    icon: "rack",
    key: "F6",
    agentName: "channel rack",
    keys: true,
  },
  {
    id: "piano",
    label: tk("shell.dock.piano.label"),
    navLabel: tk("shell.nav.piano.label"),
    icon: "piano",
    key: "F7",
    agentName: "piano roll",
    keys: true,
  },
  { id: "voice", label: tk("panel.voice"), navLabel: tk("panel.voice"), icon: "mic", key: "F8", agentName: "voice to notes", keys: false },
  { id: "drums", label: tk("panel.drums"), navLabel: tk("panel.drums"), icon: "drum", key: "F4", agentName: "drums", keys: false },
  { id: "mixer", label: tk("panel.mixer"), navLabel: tk("panel.mixer"), icon: "mixer", key: "F9", agentName: "mixer", keys: false },
  { id: "score", label: tk("panel.score"), navLabel: tk("panel.score"), icon: "score", key: "F10", agentName: "score", keys: true },
];

/** The tab with this id (the first, the channel rack, for an unknown one). */
/** function dockInfo(id: String) => DockInfo */
export function dockInfo(id) {
  for (const d of DOCKS) {
    if (d.id === id) return d;
  }
  return DOCKS[0];
}

/** The id of the tab a function key opens ("" for none). */
/** function dockForKey(key: String) => String */
export function dockForKey(key) {
  for (const d of DOCKS) {
    if (d.key === key) return d.id;
  }
  return "";
}
