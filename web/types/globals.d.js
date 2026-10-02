// Global type aliases for the Rosaclef frontend (loaded with `inty --lib`).
//
// The project model mirrors crates/core/src/model.rs. On the wire `params`
// and `options` are JSON objects; in memory they are lists of key/value
// entries so that they can be typed without a dictionary type.

// ------------------------------------------------------------------ newtypes
// Nominal index types: integers with different meanings must not be mixed up.
// Construct and unwrap them with the casts exported by "#brands".

/** Index into mixer.inserts (0 = master). */
/** nominal type InsertIx = Int */
/** Index into playlist.tracks. */
/** nominal type TrackIx = Int */
/** Index into a pattern's notes. */
/** nominal type NoteIx = Int */
/** Index into playlist.clips. */
/** nominal type ClipIx = Int */
/** A UI backend element handle. */
/** nominal type Handle = Int */
/** Index into a description buffer. */
/** nominal type NodeIx = Int */
/** Index into project.automation. */
/** nominal type LaneIx = Int */
/** Index into an automation lane's points. */
/** nominal type PointIx = Int */

// ------------------------------------------------------------------ model

/** type KV = { key: String, value: Number } */
/** type KS = { key: String, value: String } */
/** type Device = { type: String, enabled: Boolean, params: KV[], options: KS[] } */
/** A channel's arpeggiator; `on` false = none (only written to the project when on).
 * Held notes play `chord` above them over `octaves` octaves, one every `rate` beats. */
/** type Arp = { on: Boolean, chord: String, octaves: Int, rate: Number, direction: String, gate: Number, mode: String } */
/** A channel; `layerOf` is the id of the channel whose notes it also plays ("" = none). */
/** type Channel = { id: String, name: String, color: String, instrument: Device, volume: Number, pan: Number, mute: Boolean, mixer: InsertIx, arp: Arp, layerOf: String } */
/** type Note = { channel: String, pitch: Number, start: Number, length: Number, velocity: Number } */
/** type Pattern = { id: String, name: String, color: String, length: Number, notes: Note[] } */
/** type Track = { name: String, mute: Boolean } */
/** type Clip = { pattern: String, sample: String, track: TrackIx, start: Number, length: Number, offset: Number, gain: Number, mixer: InsertIx } */
/** type Playlist = { tracks: Track[], clips: Clip[] } */
/** type Insert = { name: String, volume: Number, pan: Number, mute: Boolean, solo: Boolean, effects: Device[] } */
/** type Mixer = { inserts: Insert[] } */
/** type Meta = { title: String, author: String, description: String } */
/** type Meter = { bar: Number, numerator: Number, denominator: Number } */
/** `transpose`: semitones the pitched instruments sound away from the written notes (-12..12). */
/** type Transport = { bpm: Number, beatsPerBar: Number, swing: Number, transpose: Int, meters: Meter[] } */
/** type AutomationPoint = { beat: Number, value: Number, curve: Number } */
/** type AutomationLane = { id: String, name: String, target: String, color: String, mute: Boolean, points: AutomationPoint[] } */
/** A colored passage of the score: song beats, or beats of `pattern` when set; `channels` empty = every staff. */
/** type ScoreMark = { start: Number, end: Number, color: String, label: String, pattern: String, channels: String[] } */
/** Sheet-music settings (project.score): key ("" = auto), hidden channels and tracks, clef per channel, colored passages. */
/** type ScoreSettings = { key: String, hidden: String[], hiddenTracks: TrackIx[], clefs: KS[], marks: ScoreMark[] } */
/** An ending of a repeat (a volta): song beats, played only on `passes` (from 1). */
/** type Ending = { start: Number, end: Number, passes: Int[] } */
/** A repeated passage of the arrangement: song beats, played `times` times in all. */
/** type Repeat = { start: Number, end: Number, times: Int, endings: Ending[] } */
/** A visual effect of the film (vignette, spotlight, focus, glow) at an amount of 0..1. */
/** type FilmEffect = { type: String, amount: Number } */
/** Where the camera drifts to by the end of a shot: NaN (an empty offset) = as at its start. */
/** type CameraMove = { zoom: Number, tilt: Number, turn: Number, offset: Number[] } */
/** A shot of the film (project.animation.shots), in song beats: unset numbers are NaN and unset names "", so their defaults apply. */
/** type Shot = { start: Number, end: Number, label: String, focus: String[], role: String, frame: String, zoom: Number, tilt: Number, turn: Number, offset: Number[], at: Number, to: CameraMove, transition: String, glide: Number, ease: String, effects: FilmEffect[] } */
/** The film of the song (project.animation): a camera over the score's pages; `on` false = none written. */
/** type Animation = { on: Boolean, mode: String, view: String, surface: String, energy: Number, effects: FilmEffect[], shots: Shot[] } */
/** A sheet of the film's 3D scene: a page (or a sharper band over one) as four desk points (top left, top right, bottom right, bottom left), its color and ink-height bitmaps (object URLs; "" while missing), the bitmaps' pixels a point and the page's turn (degrees). */
/** type GlSheet = { quad: Number[], color: String, height: String, scale: Number, rot: Number, page: Boolean } */
/**
 * A frame of the film for the renderer (web/lib/filmgl.js): its size in pixels;
 * the camera [x, y, span, tilt, turn, blur]; the desk's bounds [x0, y0, x1, y1],
 * color, texture and tile size (points); the paper's color; the sheets; glowing
 * notes [x, y, radius, amount]…; the spotlight [x, y, rx, ry, amount]; the lamp
 * [x, y, height]; effects [vignette, focus, glow]; the ink [relief, gloss, shine];
 * a seed for the grain. Desk units are points.
 */
/** type GlFrame = { width: Number, height: Number, cam: Number[], desk: Number[], deskColor: String, deskTex: String, deskTile: Number, paper: String, sheets: GlSheet[], sparks: Number[], spot: Number[], light: Number[], fx: Number[], ink: Number[], seed: Number } */
/** An encoded film: its object URL and its codecs ("AVC + AAC"). */
/** type Encoded = { url: String, codecs: String } */
/** type Project = { format: String, meta: Meta, transport: Transport, channels: Channel[], patterns: Pattern[], playlist: Playlist, mixer: Mixer, automation: AutomationLane[], score: ScoreSettings, repeats: Repeat[], animation: Animation } */

/** type Issue = { severity: String, path: String, message: String } */

// ------------------------------------------------------------ voice to notes
// A take analyzed by GET /api/transcribe (crates/studio/src/transcribe.rs):
// seconds and fractional MIDI pitches, before quantizing (web/src/voice.js).

/** type VoiceNote = { start: Number, end: Number, pitch: Number, velocity: Number } */
/** type VoiceHit = { time: Number, strength: Number, velocity: Number, kind: String } */
/** `details` holds the notes at each detail level (smoothest first); `notes` is the default level. */
/** type Take = { mode: String, duration: Number, step: Number, level: Number[], contour: Number[], notes: VoiceNote[], details: VoiceNote[][], hits: VoiceHit[] } */
/** A note the Voice panel places: `lane` is "melody" or a drum ("kick", "snare", "hat"); `raw` is where it was sung (beats); `src` indexes the take's notes or hits. */
/** type Placed = { lane: String, pitch: Number, start: Number, length: Number, velocity: Number, raw: Number, src: Int } */

// ---------------------------------------------------------------- catalog

/** type ParamSpec = { key: String, label: String, min: Number, max: Number, default: Number, unit: String, curve: String, integer: Boolean, doc: String } */
/** type OptionSpec = { key: String, label: String, choices: String[], choiceDocs: String[], default: String, doc: String } */
/** type DeviceSpec = { type: String, label: String, category: String, doc: String, bestFor: String, params: ParamSpec[], options: OptionSpec[], openParams: Boolean } */
/** type PluginInfo = { format: String, path: String, id: String, name: String, vendor: String, version: String, description: String, features: String[], instrument: Boolean, effect: Boolean } */
/** type PresetInfo = { name: String, type: String, tags: String, doc: String, params: KV[], options: KS[] } */
/** type ArpCatalog = { chords: String[], directions: String[], modes: String[], rateMin: Number, rateMax: Number, gateMin: Number, gateMax: Number, octavesMax: Int } */
/** type Catalog = { devices: DeviceSpec[], plugins: PluginInfo[], presets: PresetInfo[], arp: ArpCatalog } */
/** A resolved automation target (web/src/automation.js). `kind`: tempo, swing, gain, pan or param; `open`: plugin parameter without a known range. */
/** type TargetInfo = { ok: Boolean, kind: String, spec: ParamSpec, base: Number, label: String, color: String, open: Boolean } */
/** type AgentPreset = { id: String, name: String, command: String[], available: Boolean, hint: String } */

// --------------------------------------------------------------- platform

/** type Ev = {
    clientX: Number, clientY: Number, offsetX: Number, offsetY: Number,
    movementX: Number, movementY: Number, button: Number, buttons: Number, pointerId: Number, pointerType: String,
    deltaX: Number, deltaY: Number, key: String, code: String,
    shiftKey: Boolean, ctrlKey: Boolean, metaKey: Boolean, altKey: Boolean, repeat: Boolean,
    detail: Number, typing: Boolean, onControl: Boolean, value: String, checked: Boolean,
    targetLeft: Number, targetTop: Number, targetWidth: Number, targetHeight: Number,
    scrollLeft: Number, scrollTop: Number,
    preventDefault: () => Undefined, stopPropagation: () => Undefined
} */

/** type Gradient = { addColorStop: (Number, String) => Undefined } */

/** type Ctx = {
    fillStyle: String, strokeStyle: String, lineWidth: Number, font: String,
    textAlign: String, textBaseline: String, globalAlpha: Number,
    shadowColor: String, shadowBlur: Number, lineCap: String, lineJoin: String,
    fillRect: (Number, Number, Number, Number) => Undefined,
    strokeRect: (Number, Number, Number, Number) => Undefined,
    clearRect: (Number, Number, Number, Number) => Undefined,
    beginPath: () => Undefined, closePath: () => Undefined,
    moveTo: (Number, Number) => Undefined, lineTo: (Number, Number) => Undefined,
    rect: (Number, Number, Number, Number) => Undefined,
    roundRect: (Number, Number, Number, Number, Number) => Undefined,
    arc: (Number, Number, Number, Number, Number) => Undefined,
    quadraticCurveTo: (Number, Number, Number, Number) => Undefined,
    fill: () => Undefined, stroke: () => Undefined, clip: () => Undefined,
    fillText: (String, Number, Number) => Undefined,
    measureText: (String) => { width: Number },
    save: () => Undefined, restore: () => Undefined,
    translate: (Number, Number) => Undefined, scale: (Number, Number) => Undefined,
    setLineDash: (Number[]) => Undefined,
    createLinearGradient: (Number, Number, Number, Number) => Gradient,
    fillGradient: (Gradient) => Undefined,
    strokeGradient: (Gradient) => Undefined
} */

/** A PDF object: its dictionary and, for a stream, its content (compressed when written). */
/** type PdfObj = { head: String, stream: String } */

/** type Bytes = { byteLength: Number } */
/** type Floats = { length: Number } */
/** type FileRef = { name: String, size: Number } */

/** type RawSock = { send: (String) => Undefined, close: () => Undefined, isOpen: () => Boolean } */
/** type SockHandlers = { onOpen: () => Undefined, onText: (String) => Undefined, onBinary: (Bytes) => Undefined, onClose: () => Undefined } */

/** type Term = {
    write: (Bytes) => Undefined, writeText: (String) => Undefined,
    fit: () => Undefined, cols: () => Number, rows: () => Number,
    focus: () => Undefined, clear: () => Undefined, reset: () => Undefined
} */

/** type PresetRef = { font: String, bank: Number, program: Number } */
/** type AudioMsg = { t: String, position: Number, playing: Boolean, loopLength: Number, meters: Number[], missing: String[], presets: PresetRef[], message: String, sampleRate: Number } */

/** type Decoded = { sampleRate: Number, channels: Floats[], duration: Number } */

/** Bytes of the browser's storage in use, and available (0 when unknown). */
/** type StorageUse = { usage: Number, quota: Number } */

// ------------------------------------------------------------- ui library
// See web/src/ui/tree.js. Descriptions are flat (parent index, not nested
// children), which keeps them cheap to rebuild and easy to type.

/** type Listener = { event: String, fn: (Ev) => Undefined } */
/** type Painter = (Ctx, Number, Number) => Undefined */
/** type Desc = { parent: NodeIx, type: String, key: String, cls: String, text: String, attrs: KS[], styles: KS[], props: KS[], on: Listener[], paint: Painter, canvas: Boolean } */

/** type Builder = {
    open: (String, String, String) => Undefined,
    close: () => Undefined,
    leaf: (String, String, String, String) => Undefined,
    text: (String) => Undefined,
    attr: (String, String) => Undefined,
    style: (String, String) => Undefined,
    prop: (String, String) => Undefined,
    on: (String, (Ev) => Undefined) => Undefined,
    canvas: (String, String, Painter) => Undefined,
    nodes: () => Desc[]
} */

/** type Backend = {
    create: (String) => Handle,
    root: () => Handle,
    setText: (Handle, String) => Undefined,
    setClass: (Handle, String) => Undefined,
    setAttr: (Handle, String, String) => Undefined,
    removeAttr: (Handle, String) => Undefined,
    setStyle: (Handle, String, String) => Undefined,
    setProp: (Handle, String, String) => Undefined,
    append: (Handle, Handle) => Undefined,
    remove: (Handle) => Undefined,
    listen: (Handle, String, (Ev) => Undefined) => Undefined,
    paint: (Handle, Painter) => Undefined,
    frame: (() => Undefined) => Undefined
} */
