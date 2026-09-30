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
/** type Channel = { id: String, name: String, color: String, instrument: Device, volume: Number, pan: Number, mute: Boolean, mixer: InsertIx } */
/** type Note = { channel: String, pitch: Number, start: Number, length: Number, velocity: Number } */
/** type Pattern = { id: String, name: String, color: String, length: Number, notes: Note[] } */
/** type Track = { name: String, mute: Boolean } */
/** type Clip = { pattern: String, sample: String, track: TrackIx, start: Number, length: Number, offset: Number, gain: Number, mixer: InsertIx } */
/** type Playlist = { tracks: Track[], clips: Clip[] } */
/** type Insert = { name: String, volume: Number, pan: Number, mute: Boolean, solo: Boolean, effects: Device[] } */
/** type Mixer = { inserts: Insert[] } */
/** type Meta = { title: String, author: String, description: String } */
/** type Transport = { bpm: Number, beatsPerBar: Number, swing: Number } */
/** type AutomationPoint = { beat: Number, value: Number, curve: Number } */
/** type AutomationLane = { id: String, name: String, target: String, color: String, mute: Boolean, points: AutomationPoint[] } */
/** type Project = { format: String, meta: Meta, transport: Transport, channels: Channel[], patterns: Pattern[], playlist: Playlist, mixer: Mixer, automation: AutomationLane[] } */

/** type Issue = { severity: String, path: String, message: String } */

// ---------------------------------------------------------------- catalog

/** type ParamSpec = { key: String, label: String, min: Number, max: Number, default: Number, unit: String, curve: String, integer: Boolean, doc: String } */
/** type OptionSpec = { key: String, label: String, choices: String[], default: String, doc: String } */
/** type DeviceSpec = { type: String, label: String, category: String, doc: String, params: ParamSpec[], options: OptionSpec[], openParams: Boolean } */
/** type PluginInfo = { format: String, path: String, id: String, name: String, vendor: String, version: String, description: String, features: String[], instrument: Boolean, effect: Boolean } */
/** type PresetInfo = { name: String, type: String, tags: String, doc: String, params: KV[], options: KS[] } */
/** type Catalog = { devices: DeviceSpec[], plugins: PluginInfo[], presets: PresetInfo[] } */
/** A resolved automation target (web/src/automation.js). `kind`: tempo, swing, gain, pan or param; `open`: plugin parameter without a known range. */
/** type TargetInfo = { ok: Boolean, kind: String, spec: ParamSpec, base: Number, label: String, color: String, open: Boolean } */
/** type AgentPreset = { id: String, name: String, command: String[], available: Boolean, hint: String } */

// --------------------------------------------------------------- platform

/** type Ev = {
    clientX: Number, clientY: Number, offsetX: Number, offsetY: Number,
    movementX: Number, movementY: Number, button: Number, buttons: Number, pointerId: Number,
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

/** type AudioMsg = { t: String, position: Number, playing: Boolean, loopLength: Number, meters: Number[], missing: String[], message: String, sampleRate: Number } */

/** type Decoded = { sampleRate: Number, channels: Floats[], duration: Number } */

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
