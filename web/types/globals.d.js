// Global type aliases for the Rosaclef frontend (loaded with `inty --lib`,
// after the UI library's own, web/tree/types.d.js: Ev, Ctx, KS, Builder, ...).
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
/** Index into project.automation. */
/** nominal type LaneIx = Int */
/** Index into an automation lane's points. */
/** nominal type PointIx = Int */

// ------------------------------------------------------------------ model

/** type KV = { key: String, value: Number } */
/** type Device = { type: String, enabled: Boolean, params: KV[], options: KS[] } */
/** A channel's arpeggiator; `on` false = none (only written to the project when on).
 * Held notes play `chord` above them over `octaves` octaves, one every `rate` beats. */
/** type Arp = { on: Boolean, chord: String, octaves: Int, rate: Number, direction: String, gate: Number, mode: String } */
/** A channel; `layerOf` is the id of the channel whose notes it also plays ("" = none). */
/** type Channel = { id: String, name: String, color: String, instrument: Device, volume: Number, pan: Number, mute: Boolean, mixer: InsertIx, arp: Arp, layerOf: String } */
/** type Note = { channel: String, pitch: Number, start: Number, length: Number, velocity: Number } */
/** A drum pattern's recipe (the Drums tab): `on` false = an ordinary pattern. */
/** type PatternDrums = { on: Boolean, groove: String, play: String, fill: String, crash: Boolean, turnaround: Boolean, kit: String, feel: String, swing: Number, seed: Int, edited: Boolean } */
/** type Pattern = { id: String, name: String, color: String, length: Number, notes: Note[], drums: PatternDrums } */
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
/** A visual effect of the film (vignette, spotlight, glow) at an amount of 0..1. */
/** type FilmEffect = { type: String, amount: Number } */
/** Where the camera drifts to by the end of a shot: NaN (an empty offset) = as at its start. */
/** type CameraMove = { zoom: Number, tilt: Number, turn: Number, offset: Number[] } */
/** A shot of the film (project.animation.shots), in song beats: unset numbers are NaN and unset names "", so their defaults apply. */
/** type Shot = { start: Number, end: Number, label: String, focus: String[], role: String, frame: String, zoom: Number, tilt: Number, turn: Number, offset: Number[], at: Number, to: CameraMove, transition: String, glide: Number, ease: String, effects: FilmEffect[] } */
/** The film of the song (project.animation): a camera over the score's pages; `on` false = none written. */
/** type Animation = { on: Boolean, mode: String, view: String, surface: String, energy: Number, effects: FilmEffect[], shots: Shot[] } */
/** A sheet of the film's 3D scene: a page (or a sharper band over one) as four desk points (top left, top right, bottom right, bottom left), its bitmap (an object URL; "" while missing), the part of its page it shows ([x, y, w, h], points), the bitmap's pixels a point and the page's turn (degrees). */
/** type GlSheet = { quad: Number[], color: String, box: Number[], scale: Number, rot: Number, page: Boolean } */
/**
 * A frame of the film for the renderer (web/lib/filmgl.js): its size in pixels;
 * the camera [x, y, span, tilt, turn]; the desk's bounds [x0, y0, x1, y1],
 * color, texture and tile size (points); the paper's color, its texture's tiles
 * (tooth, formation, grain: image URLs) and their sizes (points), and the
 * page [width, height, staff space] (points); the sheets; the notes playing
 * [x, y, notehead half-width, brightness]… (up to 48); the spotlight [x, y, rx, ry, amount]; the lamp
 * [x, y, height]; effects [vignette, glow]; a seed for the grain; whether to
 * lay the finish over it (warmth, soft highlights, grain); and the most pixels a
 * CSS pixel on screen. No paper tiles: plain paper. Desk units are points.
 */
/** type GlFrame = { width: Number, height: Number, cam: Number[], desk: Number[], deskColor: String, deskTex: String, deskTile: Number, paper: String, paperTex: String[], paperSize: Number[], pageSize: Number[], sheets: GlSheet[], sparks: Number[], spot: Number[], light: Number[], fx: Number[], seed: Number, finish: Boolean, ratio: Number } */
/** An encoded film: its object URL and its codecs ("AVC + AAC"). */
/** type Encoded = { url: String, codecs: String } */
// The drum part (crates/core/src/drums): `on` false = the project has none.
/** type DrumSection = { name: String, bars: Number, play: String, fill: String, crash: Boolean, groove: String } */

// A groove changed for the song: its parts' [drum, steps] rows.
/** type GrooveEdit = { groove: String, a: String[][], b: String[][] } */

// A pattern edited by hand, kept note for note: notes by drum role.
/** type KeptNote = { role: String, start: Number, length: Number, velocity: Number } */

/** type KeptPattern = { slot: String, name: String, notes: KeptNote[] } */

/** type WrittenRef = { id: String, slot: String, print: String } */

/** type DrumPart = { on: Boolean, groove: String, kit: String, feel: String, swing: Number, start: Number, ending: String, variations: Boolean, seed: Number, sections: DrumSection[], grooves: GrooveEdit[], kept: KeptPattern[], written: WrittenRef[] } */

/** What the Critic runs and leaves out (project.critic): checks turned off and on (rule ids), findings suppressed (keys). */
/** type CriticSettings = { off: String[], on: String[], suppress: String[] } */
/** type Project = { format: String, meta: Meta, transport: Transport, channels: Channel[], patterns: Pattern[], playlist: Playlist, mixer: Mixer, automation: AutomationLane[], score: ScoreSettings, repeats: Repeat[], animation: Animation, drums: DrumPart, critic: CriticSettings } */

/** type Issue = { severity: String, path: String, message: String } */

// ------------------------------------------------------------------ critic
// The Critic's findings (POST /api/critic, crates/core/src/critic).

/** Where a finding points: kind "pattern" (`id`; `channel` and `notes` when set), "channel", "insert" (`index`),
 * "song" (`beat`; `index` = a clip or -1), "lane" (`index`) or "project". */
/** type Where = { kind: String, id: String, channel: String, index: Int, beat: Number, notes: Int[], label: String } */
/** A finding. `level`: "warn" or "info". `fix`: the label of its one-click fix ("" = an issue only). */
/** type Finding = { key: String, rule: String, category: String, level: String, title: String, detail: String, where: Where, fix: String, suppressed: Boolean } */
/** A check the Critic runs. */
/** type Rule = { id: String, category: String, name: String, why: String, defaultOn: Boolean } */

// The groove library (GET /api/grooves): rows are [role, steps] pairs.
/** type GrooveInfo = { id: String, style: String, name: String, meter: String, barBeats: Int, steps: Int, tempo: Int[], kit: String, swing: Number, a: String[][], b: String[][] } */

/** type GrooveCatalog = { grooves: GrooveInfo[], kits: String[] } */

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
// A sample collection the instruments play (the soundfont): its provenance and license.
/** type GmPreset = { name: String, bank: Int, program: Int } */

/** type SampleCollection = { id: String, name: String, version: String, license: String, authors: String, summary: String, source: String, licenseFile: String, readmeFile: String, sourcesFile: String, instrument: String, presets: GmPreset[] } */

/** An instrument to add, try or swap in (ui/instruments.js): `key` names
 * the browser item it comes from, `name` the channel it would make. */
/** type Pick = { key: String, name: String, device: Device } */
/** Who the piano plays (ui/instruments.js): a channel's id (or the audition
 * channel's), its name and color, what instrument it is, and whether it is
 * only being tried. */
/** type KeysTarget = { id: String, name: String, color: String, detail: String, trying: Boolean } */
/** type Catalog = { devices: DeviceSpec[], plugins: PluginInfo[], presets: PresetInfo[], arp: ArpCatalog, collections: SampleCollection[] } */
/** A resolved automation target (web/src/automation.js). `kind`: tempo, swing, gain, pan or param; `open`: plugin parameter without a known range. */
/** type TargetInfo = { ok: Boolean, kind: String, spec: ParamSpec, base: Number, label: String, color: String, open: Boolean } */
/** How a control records its changes (web/src/ui/widgets.js): a gesture is
 * `begin`, then `change` at each step (one undo step); `commit` makes one of a
 * single change. The store's `projectEdit` records project edits. */
/** type Edit = { begin: () => Undefined, change: () => Undefined, commit: (() => Undefined) => Undefined } */
/** type AgentPreset = { id: String, name: String, command: String[], available: Boolean, hint: String } */

// --------------------------------------------------------------- platform

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
/** type AudioMsg = { t: String, position: Number, playing: Boolean, loopLength: Number, meters: Number[], missing: String[], presets: PresetRef[], message: String, sampleRate: Number, stale: Boolean } */

/** type Decoded = { sampleRate: Number, channels: Floats[], duration: Number } */

/** Bytes of the browser's storage in use, and available (0 when unknown). */
/** type StorageUse = { usage: Number, quota: Number } */

// ---------------------------------------------------------------- mix check
// The report of POST /api/mixcheck (crates/studio/src/mixcheck, docs/mixcheck.md),
// as web/src/ui/mixcheck.js decodes it: a missing number is NaN, a JSON Patch is kept as JSON text.

/** A compressor's or limiter's gain reduction (dB) over the range: `above3` = % of the time over 3 dB. */
/** type MixGr = { id: String, effect: Int, kind: String, max: Number, mean: Number, above3: Number } */
/** A part's share (%) of the mix's energy. */
/** type MixShare = { id: String, pct: Number } */
/** A row of the report (a bar, a section or N beats; `pass` 0 = not a repeat). */
/** type MixRow = { label: String, bar: Int, pass: Int, fromBeat: Number, toBeat: Number, lufs: Number, mMax: Number, sMax: Number, peak: Number, truePeak: Number, preLimiter: Number, limGr: Number, corr: Number, spectrum: Number[], top: MixShare[] } */
/** type MixMasker = { id: String, lo: Number, hi: Number, db: Number } */
/** A change the mix check suggests: `patch` is JSON Patch text; `verified` a summary once re-measured. */
/** type MixSuggestion = { why: String, patch: String, expRel: Number, expAud: Number, verified: String } */
/** type MixElement = { id: String, name: String, kind: String, role: String, lead: Boolean, anchor: String, range: Number[], buriedIn: { from: Int, to: Int, rel: Number }[], insert: Int, rms: Number, peak: Number, lufs: Number, rel: Number, share: Number, active: Number, corr: Number, audible: Number, maskers: MixMasker[], domLo: Number, domHi: Number, fader: Number, verdict: String, suggestions: MixSuggestion[] } */
/** type MixFinding = { severity: String, rule: String, key: String, where: String, detail: String, element: String, fromBar: Int, toBar: Int, fromBeat: Number, patch: String, label: String } */
/** type MixMaster = { integrated: Number, shortMax: Number, momentaryMax: Number, truePeak: Number, samplePeak: Number, rms: Number, preLimiter: Number, preEffects: Number, limGr: MixGr, compGr: MixGr, plr: Number, crest: Number, lra: Number, corrMean: Number, corrMin: Number, monoLoss: Number, spectrum: Number[] } */
/** A delivery target's verdict: `status` pass, warn or fail; `gain` what the platform applies (dB). */
/** type MixTarget = { id: String, name: String, lufs: Number, truePeak: Number, status: String, gain: Number, notes: String[] } */
/** The master over time: momentary and short-term LUFS, true peak (dBTP) and limiter GR (dB) every `step` seconds. */
/** type MixHistory = { step: Number, t: Number[], m: Number[], s: Number[], tp: Number[], gr: Number[], bars: { t: Number, bar: Int, pass: Int, beat: Number }[] } */
/** A reference recording, level-matched: `spectrumDiff` = the mix minus the reference per band (dB). */
/** type MixReference = { file: String, levelMatch: Number, spectrumDiff: Number[], summary: String, master: MixMaster } */
/** type MixReport = { ok: Boolean, fromBar: Int, toBar: Int, fromBeat: Number, toBeat: Number, seconds: Number, repeats: Boolean, master: MixMaster, target: MixTarget, reference: MixReference, rows: MixRow[], elements: MixElement[], gr: MixGr[], findings: MixFinding[], history: MixHistory, whatIf: String, cached: Boolean, ms: Number, renders: Int, warnings: String[] } */
/** type MixTargetInfo = { id: String, name: String, lufs: Number, truePeak: Number } */
/** How far an export or a mix check under way has come (see crates/studio/src/jobs.rs). */
/** type Job = { id: Int, active: Boolean, what: String, stage: String, done: Number, render: Int, seconds: Number, total: Number, state: String } */
