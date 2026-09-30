// Type declarations for web/lib/platform.js — the small, unchecked FFI layer
// between inty-checked application code and browser APIs that inty's
// standard library does not describe yet (typed events, canvas, sockets,
// Web Audio, xterm.js). `inty.json` maps the "platform" import here.

/** const listen: <E>(E, String, (Ev) => Undefined) => Undefined */
export const listen;

/** const listenWindow: (String, (Ev) => Undefined) => Undefined */
export const listenWindow;

/** const capturePointer: <E>(E, Number) => Undefined */
export const capturePointer;

/** const canvas2d: <E>(E, Number, Number) => Ctx */
export const canvas2d;

/** const now: () => Number */
export const now;

/** const connectRaw: (String, SockHandlers) => RawSock */
export const connectRaw;

/** const getJson: <T>(String) => Promise<T> */
export const getJson;

/** const sendJson: <B, T>(String, String, B) => Promise<T> */
export const sendJson;

/** const uploadFile: <T>(String, FileRef) => Promise<T> */
export const uploadFile;

/** "server" (the Rosaclef server) or "local" (the browser-only studio). */
/** const backendMode: () => Promise<String> */
export const backendMode;

/** const storageEstimate: () => Promise<StorageUse> */
export const storageEstimate;

/** const onFileDrop: <E>(E, (FileRef[], Number, Number) => Undefined) => Undefined */
export const onFileDrop;

/** const pickFiles: (String, (FileRef[]) => Undefined) => Undefined */
export const pickFiles;

/** const download: (String, String) => Undefined */
export const download;

/** const previewAudio: (String, () => Undefined) => Undefined */
export const previewAudio;

/** const stopPreview: () => Undefined */
export const stopPreview;

/** const fmtDate: (Number) => String */
export const fmtDate;

/** const nowIso: () => String */
export const nowIso;

/** const wsUrl: (String) => String */
export const wsUrl;

/** const createTerm: <E>(E, (String) => Undefined) => Term */
export const createTerm;

/** const audioStart: (String, String, (AudioMsg) => Undefined) => Promise<Number> */
export const audioStart;

/** const audioPost: <M>(M) => Undefined */
export const audioPost;

/** const audioPostSample: (String, Decoded) => Undefined */
export const audioPostSample;

/** const audioLoadPreset: (String, Number, Number) => Promise<Boolean> */
export const audioLoadPreset;

/** const audioResume: () => Promise<Boolean> */
export const audioResume;

/** const audioRunning: () => Boolean */
export const audioRunning;

/** const decodeAudioUrl: (String) => Promise<Decoded> */
export const decodeAudioUrl;

/** const recStart: () => Promise<Boolean> */
export const recStart;

/** const recStop: (String) => Promise<String> */
export const recStop;

/** const setTitle: (String) => Undefined */
export const setTitle;

/** const confirmBox: (String) => Boolean */
export const confirmBox;

/** const promptBox: (String, String) => String */
export const promptBox;

/** const loadPref: (String) => String */
export const loadPref;

/** const savePref: (String, String) => Undefined */
export const savePref;

/** const fmt: (Number, Number) => String */
export const fmt;

/** const drag: (Ev, (Ev) => Undefined, (Ev) => Undefined) => Undefined */
export const drag;

/** On a touch screen, runs the action only for a tap (a finger that moves scrolls instead); otherwise at once. */
/** const pressOrTap: (Ev, (Ev) => Undefined) => Undefined */
export const pressOrTap;

/** const debounce: (Number, () => Undefined) => () => Undefined */
export const debounce;

/** const domBackend: (String) => Backend */
export const domBackend;
