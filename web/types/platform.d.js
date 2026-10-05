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

/** const siteText: (String) => Promise<String> */
export const siteText;

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

/** Device pixels a CSS pixel (it follows the browser's zoom). */
/** const pixelRatio: () => Number */
export const pixelRatio;
/** Write PDF objects (the catalog first, the document info last) to a file and download it. */
/** const downloadPdf: (String, PdfObj[]) => Promise<Boolean> */
export const downloadPdf;
/** Write a PDF whose pages are images: SVG pages of a size in points, drawn at a scale in pixels a point; with its document info. */
/** const downloadImagePdf: (String, String, String[], Number, Number, Number) => Promise<Boolean> */
export const downloadImagePdf;

/** Draw an SVG document into a bitmap of a size in pixels, as an image of a type ("image/jpeg", "image/png"): its object URL (give it back with dropUrl). */
/** const rasterSvg: (String, Int, Int, String) => Promise<String> */
export const rasterSvg;
/** Whether the film draws smoothly here as it is: Chrome (or another Chromium browser) on a GPU. */
/** const fastGraphics: () => Boolean */
export const fastGraphics;
/** Let go of an object URL made by rasterSvg. */
/** const dropUrl: (String) => Undefined */
export const dropUrl;
/** Show the element matching a CSS selector full screen, or leave full screen. */
/** const toggleFullscreen: (String) => Undefined */
export const toggleFullscreen;

/** Draw a frame of the film (WebGL) on the canvas matching a CSS selector, at the next animation frame. */
/** const filmDraw: (String, GlFrame) => Undefined */
export const filmDraw;
/** Draw a canvas every animation frame, a frame from the function each time, until it gives none. */
/** const filmLive: (String, () => GlFrame | Undefined) => Undefined */
export const filmLive;
/** Forget the texture made from an object URL (before letting it go). */
/** const filmForget: (String) => Undefined */
export const filmForget;
/** Draw one frame of the film (WebGL, every bitmap it names loaded first): a PNG's object URL. */
/** const renderStill: (GlFrame) => Promise<String> */
export const renderStill;
/** Encode a film as MP4: width, height, frames a second, frames, each frame (made when asked), the mixdown (no channels: silent), where in it the film starts (seconds) and progress (0..1). */
/** const encodeFilm: (Int, Int, Int, Int, (Int) => Promise<GlFrame>, Decoded, Number, (Number) => Undefined) => Promise<Encoded> */
export const encodeFilm;

/** Width of a text in a PDF standard font ("Times-Italic", …) at size 1, measured with a metric-compatible face. */
/** const textWidth: (String, String) => Number */
export const textWidth;

/** Take the keyboard focus from whatever holds it (a text field, a menu, a button, the terminal): the studio's keys work again. */
/** const releaseFocus: () => Undefined */
export const releaseFocus;

/** "letter" where US Letter is the paper size (the US and Canada), else "a4". */
/** const paperSize: () => String */
export const paperSize;

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
