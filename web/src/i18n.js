// The interface's language: every text the studio shows goes through t() (or
// tf() when it has values in it), keyed by its English wording. The
// translations are JSON files in web/locales/ (English text → translated
// text); web/locales/en.json lists every text, and tools/i18n.mjs keeps it in
// step with the code (docs/i18n.md).

import { loadMessages, clearMessages, message, setUiLocale, browserLanguages, loadPref, savePref } from "#platform";

/** A language the studio speaks: its BCP 47 tag, its name in itself and in English. */
/** type Language = { code: String, name: String, english: String } */

/** The ten languages most used in music production, English first. */
/** const LANGUAGES: Language[] */
export const LANGUAGES = [
  { code: "en", name: "English", english: tk("English") },
  { code: "es", name: "Español", english: tk("Spanish") },
  { code: "pt-BR", name: "Português (Brasil)", english: tk("Portuguese (Brazil)") },
  { code: "fr", name: "Français", english: tk("French") },
  { code: "de", name: "Deutsch", english: tk("German") },
  { code: "it", name: "Italiano", english: tk("Italian") },
  { code: "ja", name: "日本語", english: tk("Japanese") },
  { code: "ko", name: "한국어", english: tk("Korean") },
  { code: "zh-CN", name: "简体中文", english: tk("Chinese (Simplified)") },
  { code: "ru", name: "Русский", english: tk("Russian") },
];

const PREF = "rosaclef.language";

/** The language shown, and the one being loaded ("" = none). */
const lang = { code: "en", loading: "" };

/** A text in the interface's language. */
/** function t(text: String) => String */
export function t(text) {
  return message(text);
}

/** A text with values in it, in the interface's language: {0}, {1}… stand for the values, in order. */
/** function tf(text: String, values: String[]) => String */
export function tf(text, values) {
  let s = message(text);
  for (let i = 0; i < values.length; i++) s = s.split(`{${i}}`).join(values[i]);
  return s;
}

/** Marks a text for translation where it is written (a table of labels) and gives it back as it is: t() translates it where it is shown. */
/** function tk(text: String) => String */
export function tk(text) {
  return text;
}

/** The language shown (a code of LANGUAGES). */
/** function language() => String */
export function language() {
  return lang.code;
}

/** The language of LANGUAGES closest to a BCP 47 tag ("pt-PT" → "pt-BR", "zh-Hans-CN" → "zh-CN"), or "" for none. */
/** function matchLanguage(tag: String) => String */
export function matchLanguage(tag) {
  const want = tag.trim().toLowerCase();
  if (want === "") return "";
  for (const l of LANGUAGES) {
    if (l.code.toLowerCase() === want) return l.code;
  }
  const base = want.split("-")[0];
  for (const l of LANGUAGES) {
    if (l.code.toLowerCase().split("-")[0] === base) return l.code;
  }
  return "";
}

/** Show the interface in a language (a code of LANGUAGES) and remember it; `done` runs once its texts are in. */
/** function setLanguage(code: String, done: () => Undefined) => Undefined */
export function setLanguage(code, done) {
  const c = matchLanguage(code);
  const next = c === "" ? "en" : c;
  savePref(PREF, next);
  useLanguage(next, done);
}

/** function useLanguage(code: String, done: () => Undefined) => Undefined */
function useLanguage(code, done) {
  if (code === "en") {
    lang.loading = "";
    lang.code = "en";
    clearMessages();
    setUiLocale("en");
    done();
    return undefined;
  }
  lang.loading = code;
  loadMessages(`locales/${code}.json`)
    .then((n) => {
      // A later choice wins over this one.
      if (lang.loading !== code) return false;
      lang.loading = "";
      lang.code = code;
      setUiLocale(code);
      done();
      return true;
    })
    .catch((e) => {
      if (lang.loading === code) lang.loading = "";
      return false;
    });
}

/** At start: the language chosen before, else the browser's (when the studio speaks it), else English. */
/** function loadLanguage(done: () => Undefined) => Undefined */
export function loadLanguage(done) {
  let code = matchLanguage(loadPref(PREF));
  if (code === "") {
    for (const tag of browserLanguages()) {
      if (code === "") code = matchLanguage(tag);
    }
  }
  useLanguage(code === "" ? "en" : code, done);
}
