#!/usr/bin/env node
// The studio's translations (docs/i18n.md). Every text the interface shows has
// a semantic key ("score.ribbon.gloss.label") written as a literal in t("…"),
// tf("…", [values]) or tk("…") in web/src; web/locales/en.json gives each key
// its English, and every other locale its translation.
//
//   node tools/i18n.mjs            report what is missing or left over
//   node tools/i18n.mjs --check    fail if a key the code uses has no English, a
//                                  key in en.json is not used, a locale misses a
//                                  key or keeps one en.json lacks, or a
//                                  translation's {0} placeholders differ
//   node tools/i18n.mjs --prune    drop, from every locale and en.json, the keys
//                                  the code no longer uses

import { readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join, dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const web = join(root, "web");
const localesDir = join(web, "locales");

/** A key: dot-separated lowerCamelCase segments, the first naming the module. */
export const KEY = /^[a-z][a-zA-Z0-9]*(\.[a-z0-9][a-zA-Z0-9]*)+$/;

/** Every .js file under a folder, sorted. */
function jsFiles(dir) {
  const out = [];
  for (const name of readdirSync(dir).sort()) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...jsFiles(p));
    else if (name.endsWith(".js")) out.push(p);
  }
  return out;
}

// A string literal: "…", '…' or `…` without ${}.
const LIT = /"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\$]|\\.)*`/.source;
// t("…"), tf("…", …) and tk("…") with a literal first argument (not a method: `x.t(`).
const CALL = new RegExp(`(?<![\\w.$])(?:t|tf|tk)\\(\\s*(${LIT})`, "g");

/** The keys the code uses, with where each is first used; and the literals that are not keys. */
export function extract() {
  const keys = new Map();
  const bad = [];
  for (const file of jsFiles(join(web, "src"))) {
    const src = readFileSync(file, "utf8");
    for (const m of src.matchAll(CALL)) {
      const key = Function(`"use strict"; return (${m[1]});`)();
      const where = `${relative(root, file)}:${src.slice(0, m.index).split("\n").length}`;
      if (!KEY.test(key)) bad.push(`${where}: not a key: ${JSON.stringify(key).slice(0, 80)}`);
      else if (!keys.has(key)) keys.set(key, where);
    }
  }
  return { keys, bad };
}

/** The placeholders of a text ({0}, {1}…), sorted. */
function placeholders(s) {
  return [...s.matchAll(/\{\d+\}/g)].map((m) => m[0]).sort();
}

const readJson = (p) => JSON.parse(readFileSync(p, "utf8"));
const writeJson = (p, o) => writeFileSync(p, JSON.stringify(o, null, 2) + "\n");

const args = new Set(process.argv.slice(2));
const check = args.has("--check");
const prune = args.has("--prune");

const { keys, bad } = extract();
const problems = [...bad];
const report = (what, list) => {
  if (list.length === 0) return;
  problems.push(`${what}: ${list.length}`);
  for (const k of list.slice(0, 15)) problems.push(`    ${k}`);
  if (list.length > 15) problems.push(`    …`);
};

const enPath = join(localesDir, "en.json");
const en = readJson(enPath);
report(
  "en.json: keys the code uses without English",
  [...keys].filter(([k]) => typeof en[k] !== "string" || en[k] === "").map(([k, w]) => `${k} (${w})`)
);
const unused = Object.keys(en).filter((k) => !keys.has(k));
const sorted = Object.keys(en).some((k, i, a) => i > 0 && a[i - 1] > k);
if (prune) {
  const kept = {};
  for (const k of Object.keys(en).sort()) if (keys.has(k)) kept[k] = en[k];
  writeJson(enPath, kept);
} else {
  report("en.json: keys the code does not use (--prune drops them)", unused);
  if (sorted) problems.push("en.json: keys are not sorted (--prune sorts them)");
}
const enKeys = Object.keys(en).filter((k) => !prune || keys.has(k));

for (const name of readdirSync(localesDir).sort()) {
  if (!name.endsWith(".json") || name === "en.json") continue;
  const path = join(localesDir, name);
  const loc = readJson(path);
  const missing = enKeys.filter((k) => typeof loc[k] !== "string" || loc[k].trim() === "");
  const extra = Object.keys(loc).filter((k) => !enKeys.includes(k));
  const broken = enKeys.filter((k) => typeof loc[k] === "string" && loc[k] !== "" && placeholders(loc[k]).join() !== placeholders(en[k]).join());
  if (prune) {
    const kept = {};
    for (const k of enKeys.slice().sort()) if (typeof loc[k] === "string") kept[k] = loc[k];
    writeJson(path, kept);
  } else {
    report(`${name}: keys not translated`, missing);
    report(`${name}: keys en.json does not have (--prune drops them)`, extra);
  }
  report(`${name}: translations whose {placeholders} differ from the English`, broken);
}

console.log(`${keys.size} keys.`);
if (problems.length > 0) {
  console.log(problems.join("\n"));
  if (check) process.exit(1);
}
