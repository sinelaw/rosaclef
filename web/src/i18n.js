// The interface's language. Every text the studio shows has a semantic key
// ("score.ribbon.gloss.label"), shown with t(key), or tf(key, values) when it
// has values in it. web/locales/en.json holds the English of every key, and
// each other web/locales/<code>.json its translation; a key a language lacks
// shows in English (docs/i18n.md).

import { loadMessages, loadBaseMessages, clearMessages, message, setUiLocale, browserLanguages, loadPref, savePref } from "#platform";

/** A language the studio speaks: its BCP 47 tag, its name in itself, and the key of its name in the interface's language. */
/** type Language = { code: String, name: String, english: String } */

/** The ten languages most used in music production, English first. */
/** const LANGUAGES: Language[] */
export const LANGUAGES = [
  { code: "en", name: "English", english: tk("language.en") },
  { code: "es", name: "Español", english: tk("language.es") },
  { code: "pt-BR", name: "Português (Brasil)", english: tk("language.ptBR") },
  { code: "fr", name: "Français", english: tk("language.fr") },
  { code: "de", name: "Deutsch", english: tk("language.de") },
  { code: "it", name: "Italiano", english: tk("language.it") },
  { code: "ja", name: "日本語", english: tk("language.ja") },
  { code: "ko", name: "한국어", english: tk("language.ko") },
  { code: "zh-CN", name: "简体中文", english: tk("language.zhCN") },
  { code: "ru", name: "Русский", english: tk("language.ru") },
];

const PREF = "rosaclef.language";

/** The language shown, the one being loaded ("" = none), and whether the English (every key's fallback) is in. */
const lang = { code: "en", loading: "", base: false };

/** The text of a key in the interface's language (English when the language lacks it). */
/** function t(key: String) => String */
export function t(key) {
  return message(key);
}

/** TEMPORARY (removed once no call site uses it). */
/** function tx(context: String, text: String) => String */
export function tx(context, text) {
  return message(text);
}

/** TEMPORARY (removed once no call site uses it). */
/** function tkx(context: String, text: String) => String */
export function tkx(context, text) {
  return text;
}

/** The text of a key with values in it: {0}, {1}… in the text stand for the values, in order. */
/** function tf(key: String, values: String[]) => String */
export function tf(key, values) {
  let s = message(key);
  for (let i = 0; i < values.length; i++) s = s.split(`{${i}}`).join(values[i]);
  return s;
}

/** Marks a key where it is written (a table of labels) and gives it back: t() shows it where it is shown. */
/** function tk(key: String) => String */
export function tk(key) {
  return key;
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
      // Not loaded: the studio stays in the language it shows.
      if (lang.loading === code) lang.loading = "";
      done();
      return false;
    });
}

/** At start: the English (every key's fallback), then the language chosen before, else the browser's (when the studio speaks it), else English; `done` runs once the texts are in. */
/** function loadLanguage(done: () => Undefined) => Undefined */
export function loadLanguage(done) {
  let code = matchLanguage(loadPref(PREF));
  if (code === "") {
    for (const tag of browserLanguages()) {
      if (code === "") code = matchLanguage(tag);
    }
  }
  const chosen = code === "" ? "en" : code;
  loadBaseMessages("locales/en.json")
    .then((n) => {
      lang.base = true;
      useLanguage(chosen, done);
      return true;
    })
    .catch((e) => {
      useLanguage(chosen, done);
      return false;
    });
}
