#!/usr/bin/env node
// The studio's translations (docs/i18n.md): every text the interface shows is
// written in English inside t("…"), tf("…", [values]) or tk("…") in web/src;
// web/locales/en.json lists them all, and each other locale maps them to its
// language. A text that means different things in different places is written
// tx("context", "…"): its key is the context and the text joined by U+0004.
//
//   node tools/i18n.mjs            write web/locales/en.json from the code, and
//                                  report what each locale is missing
//   node tools/i18n.mjs --check    fail if en.json is not what the code says, or
//                                  a locale misses a text, has one the code no
//                                  longer uses, or loses a {0} placeholder
//   node tools/i18n.mjs --prune    also drop, from every locale, the texts the
//                                  code no longer uses

import { readFileSync, writeFileSync, readdirSync, statSync } from "node:fs";
import { join, dirname, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const web = join(root, "web");
const localesDir = join(web, "locales");

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

// A literal string: "…", '…' or `…` without ${}.
const LIT = /"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'|`(?:[^`\\$]|\\.)*`/.source;
// t("…"), tf("…", …) and tk("…") with a literal first argument (not a method: `x.t(`).
const CALL = new RegExp(`(?<![\\w.$])(?:t|tf|tk)\\(\\s*(${LIT})`, "g");
// tx("context", "…") and tkx("context", "…"): the key is the context and the text, joined by U+0004.
const CTX_CALL = new RegExp(`(?<![\\w.$])(?:tx|tkx)\\(\\s*(${LIT})\\s*,\\s*(${LIT})`, "g");
const CTX = "\u0004";

/** The value of a string literal of the code. */
function literal(src) {
  return Function(`"use strict"; return (${src});`)();
}

/** The text a key shows in English: a key with a context shows the text after it. */
export function english(key) {
  const i = key.indexOf(CTX);
  return i < 0 ? key : key.slice(i + 1);
}

/** The texts the code marks for translation, in order of first appearance. */
export function extract() {
  const keys = new Map();
  for (const file of jsFiles(join(web, "src"))) {
    const src = readFileSync(file, "utf8");
    const found = [];
    for (const m of src.matchAll(CALL)) found.push({ at: m.index, key: literal(m[1]) });
    for (const m of src.matchAll(CTX_CALL)) found.push({ at: m.index, key: `${literal(m[1])}${CTX}${literal(m[2])}` });
    found.sort((a, b) => a.at - b.at);
    for (const f of found) {
      if (f.key === "" || keys.has(f.key)) continue;
      keys.set(f.key, relative(web, file));
    }
  }
  return keys;
}

/** The placeholders of a text ({0}, {1}…), sorted. */
function placeholders(s) {
  return [...s.matchAll(/\{\d+\}/g)].map((m) => m[0]).sort();
}

function readJson(p) {
  return JSON.parse(readFileSync(p, "utf8"));
}

function writeJson(p, o) {
  writeFileSync(p, JSON.stringify(o, null, 2) + "\n");
}

const args = new Set(process.argv.slice(2));
const check = args.has("--check");
const prune = args.has("--prune");

const keys = extract();
const en = {};
for (const k of keys.keys()) en[k] = english(k);

const problems = [];
const enPath = join(localesDir, "en.json");
const enJson = JSON.stringify(en, null, 2) + "\n";
let enOnDisk = "";
try {
  enOnDisk = readFileSync(enPath, "utf8");
} catch (_) {
  /* not written yet */
}
if (check) {
  if (enOnDisk !== enJson) problems.push("web/locales/en.json is not up to date with the code: run `node tools/i18n.mjs`");
} else {
  writeFileSync(enPath, enJson);
}

for (const name of readdirSync(localesDir).sort()) {
  if (!name.endsWith(".json") || name === "en.json") continue;
  const path = join(localesDir, name);
  const loc = readJson(path);
  const missing = [...keys.keys()].filter((k) => typeof loc[k] !== "string" || loc[k].trim() === "");
  const unused = Object.keys(loc).filter((k) => !keys.has(k));
  const broken = [...keys.keys()].filter((k) => typeof loc[k] === "string" && loc[k] !== "" && placeholders(loc[k]).join() !== placeholders(k).join());
  if (prune && unused.length > 0) {
    const kept = {};
    for (const k of keys.keys()) if (typeof loc[k] === "string") kept[k] = loc[k];
    writeJson(path, kept);
  }
  const report = (what, list) => {
    if (list.length === 0) return;
    problems.push(`${name}: ${list.length} ${what}`);
    for (const k of list.slice(0, 10)) problems.push(`    ${JSON.stringify(k)}`);
    if (list.length > 10) problems.push(`    …`);
  };
  report("texts not translated", missing);
  if (!prune) report("texts the code no longer uses (--prune drops them)", unused);
  report("translations whose {placeholders} differ from the English", broken);
}

console.log(`${keys.size} texts to translate.`);
if (problems.length > 0) {
  console.log(problems.join("\n"));
  if (check) process.exit(1);
}
