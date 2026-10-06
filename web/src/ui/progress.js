// How far a long job (an export, a mix check) has come — what the back end
// says while it renders (crates/studio/src/jobs.rs). The page picks each
// job's id and sends it with the request (`?job=ID`), so several jobs at once
// each have their own: the server answers when asked, the browser back end
// tells as it goes. Polled while the job runs.

import { jobProgress } from "#platform";
import { invalidate } from "../store.js";

/** The jobs being followed, as last heard. */
/** const heard: Job[] */
const heard = [];

/** A new job's id (sent as `?job=ID`). */
/** function newJob() => Int */
export function newJob() {
  return 1 + Math.floor(Math.random() * 4294967294);
}

/** Follow job `id` while `busy()` holds: ask how far it has come, and redraw. */
/** function watchJob(id: Int, busy: () => Boolean) => Undefined */
export function watchJob(id, busy) {
  const tick = () => {
    if (!busy()) {
      const k = heard.findIndex((j) => j.id === id);
      if (k >= 0) heard.splice(k, 1);
      return undefined;
    }
    jobProgress(id)
      .then((j) => {
        const k = heard.findIndex((x) => x.id === id);
        if (k >= 0) heard[k] = j;
        else heard.push(j);
        invalidate();
        return true;
      })
      .catch((e) => false);
    setTimeout(() => tick(), 200);
  };
  tick();
}

/** function jobOf(id: Int) => Job */
function jobOf(id) {
  return heard.find((j) => j.id === id) ?? { id: id, active: false, what: "", stage: "", done: 0, render: 0, seconds: 0, total: 0 };
}

/** m:ss */
/** function clock(s: Number) => String */
function clock(s) {
  const m = Math.floor(s / 60);
  const r = Math.floor(s - m * 60);
  return `${m}:${r < 10 ? "0" : ""}${r}`;
}

/** What job `id` is doing, for people. */
/** function jobLabel(id: Int) => String */
export function jobLabel(id) {
  const j = jobOf(id);
  if (!j.active) return "Preparing…";
  const pct = `${Math.round(j.done * 100)}%`;
  if (j.stage === "samples") return `Loading samples ${pct}`;
  if (j.stage === "instruments") return `Loading instruments ${pct}`;
  if (j.stage === "tail") return "Letting the effects ring out…";
  if (j.stage === "measure") return "Measuring…";
  const more = j.render > 1 ? ` (render ${j.render})` : "";
  return j.total > 0 ? `Rendering ${clock(j.seconds)} of ${clock(j.total)} · ${pct}${more}` : `Rendering ${pct}${more}`;
}

/** How far job `id` has come (0–1), for a bar: the render is most of it. */
/** function jobFraction(id: Int) => Number */
export function jobFraction(id) {
  const j = jobOf(id);
  if (!j.active) return 0;
  if (j.stage === "samples" || j.stage === "instruments") return 0.05 * j.done;
  if (j.stage === "render") return 0.05 + 0.9 * j.done;
  return 0.97;
}
