# Mix check

`rosaclef mixcheck` answers mix questions with numbers, from **one render**:
*is the rhythm bass audible here? is section C overloading the master? is the
lead masked in the intro? what is the limiter doing at bar 94? which part
clashes in bar 65?* It replaces copying the project to cut bars out, stripping
the limiter by hand, soloing inserts one render at a time, and parsing WAVs
with ad-hoc scripts.

There are three ways in, and they are the same tool: the same request, the
same measurements, the same report.

| | |
|---|---|
| **Command line** (agents) | `rosaclef mixcheck [PATH] [flags]` — JSON (or `--text`) |
| **HTTP** (while the studio runs) | `POST $ROSACLEF_URL/api/mixcheck` with the flags as JSON |
| **The studio** | the **Mix check** tab of the Maestro panel |

The command line turns its flags into the request object the endpoint takes;
the studio's panel sends that object and draws the report it gets back. The
browser-only studio answers the endpoint in its worker (WebAssembly) and its
shell has `mixcheck` too.

## Asking

```sh
rosaclef mixcheck [PATH]
  --range BAR:BAR | --beats B:B | --section NAME   # default: the whole song
  --by bar|section|N-beats                          # the rows of perBar
  --focus ID[,ID…]                                  # channel ids, insert indices or names, "master"
  --checks levels,audibility,masking,dynamics,gainreduction,clashes,spectrum,stereo
  --what-if JSONPATCH|@file.json                    # RFC 6902, in memory only
  --compare A.json B.json                           # the report of B, with what changed from A
  --verify                                          # re-measure each suggestion under its patch
  --target spotify|apple|youtube|amazon|tidal|ebu-r128|atsc-a85
  --reference FILE                                  # a reference recording, level-matched
  --history                                         # the master over time (every 200 ms)
  --json | --text                                   # default --json; --text: at most 40 lines
  --max-findings N  --threshold strict|normal|loose
  --preroll BEATS  --sample-rate HZ  --no-cache  --schema
```

- `--range` takes bars as the producer says them: from 1, both ends included,
  following the meters (`transport.meters`). Where a repeat plays a written bar
  more than once, every pass is measured and each gets its own row (`pass`).
  `--beats` takes written song beats (like a clip's `start`, end excluded).
  `--section` takes a score mark's label or a drum part section's name (every
  passage with that name).
- The request object (HTTP and the panel) uses the same names:
  `{"range": "52:59", "by": "bar", "focus": ["rbass"], "whatIf": [...],
  "threshold": "strict", "maxFindings": 5, "target": "spotify",
  "reference": "samples/ref.wav", "history": true, "verify": true}`; without
  `"project"` the endpoint checks the open project. `"text": true` adds the
  summary as `text`. `{"apply": [ops]}` renders nothing: it returns
  `{"project": …}`, the project patched and validated — the panel's **Apply**.
  `GET /api/mixcheck` lists the checks, the targets and the schema.
- Exit status: 0 = no warnings, 1 = warnings, 2 = an error. Errors name the JSON
  path or the option at fault: `whatIf[0] replace /channels/9/volume: no
  element 9`, or the validation issues of a patched project.
- `project.json` is never written. A what-if is applied in memory.

## The render

One render answers every question: the engine taps every channel (after its
fader), every insert (after its effects and fader), and the master three times
(before its effects, at its limiter's input — after the limiter's own input
gain — and at the output), and the measurements are made as it plays. A
what-if is a second render; `--verify`, one per suggestion.

- Only the range is rendered, with a **pre-roll** (8 beats, and at least 3
  seconds) before each stretch: reverb and delay tails, compressor envelopes and
  notes held across the start (they are started as if they had been playing)
  are right when the range begins, and automation is evaluated at its true song
  position. A range that crosses a repeat renders the passes in playing order.
- Renders are **cached** on a hash of what makes the sound (the project without
  its cosmetic parts, the samples' sizes and dates, the stretch, the pre-roll,
  the sample rate, the version), in memory and in `.rosaclef/mixcheck/`. A
  repeated question, or another question about the same range, renders nothing
  (`"render": {"cached": true, "renders": 0}`).
- The render is the one `rosaclef render` makes (same engine, same seeds) and
  uses every core; the measurements of each chunk are made on every core too.
  The numbers are deterministic: the same song gives the same report.

## What it measures

- **Loudness** — ITU-R BS.1770-4: K-weighting, momentary (400 ms), short-term
  (3 s) and gated integrated loudness; the loudness range (EBU Tech 3342); true
  peak, 4× oversampled; sample peak, RMS, crest factor, PLR.
- **The limiter and the compressors** — every compressor's and limiter's gain
  reduction, read from the device itself: largest, mean, and the share of the
  time over 3 dB, per insert and per row. `master.preLimiterPeakDbfs` > 0 means
  the limiter is fighting.
- **Spectrum** — power in six bands (sub, bass, low mid, mid, high mid, air)
  and in the 24 critical bands the masking model uses.
- **Stereo** — phase correlation (mean, and the lowest over the loud parts) and
  what the mix loses folded to mono.
- **Each part** (`elements`) — every channel, plus inserts that are buses or
  play audio clips: its level, peak and loudness, its loudness against the mix
  (`relativeToMixDb`), its share of the mix's energy, where it plays, and its
  audibility. An insert's output is shared among its channels by their energy in
  each band: that is each channel's contribution at the master.
- **Audibility** is a masking model, not a ratio of levels: per 43 ms frame,
  each part's and the rest of the mix's critical-band powers are spread across
  bands (Schroeder's spreading function) into excitations, and Moore and
  Glasberg's partial loudness compares what the part adds to the masker's
  loudness with its loudness alone. A frame is audible when it keeps at least
  15 % of it (strict 25 %, loose 8 %); frames too quiet to hear even alone (the
  end of a decay) do not count. `audibleFractionPct` is the share of the
  part's frames that are audible; `maskedBy` names the parts with the strongest
  excitation where it is loudest, the band and by how much.
- **Clashes** — notes of two parts sounding together a minor second (or minor
  ninth…), a major seventh or a tritone apart, from the notes as they play and
  the parts' measured levels at that moment, weighed by overlap × the quieter
  part's level. Short passing tones are left out unless `--threshold strict`.
  Each names both notes (`pattern`, `noteIndex`, pitch) and the same two notes
  clashing again in a loop are one clash (`alsoInBars`).

## The report

The JSON is stable and documented by `mixcheck.schema.json` (`rosaclef
mixcheck --schema`; also written to `.rosaclef/mixcheck.schema.json` in every
project folder). dB values are rounded to 0.1, percentages to whole numbers;
silent parts are left out.

```jsonc
{
  "version": 1,
  "range": { "fromBar": 52, "toBar": 59, "fromBeat": 196, "toBeat": 228, "by": "bar", "seconds": 16.0, "threshold": "normal" },
  "master": {
    "integratedLufs": -9.8, "shortTermLufsMax": -8.1, "momentaryLufsMax": -7.2, "truePeakDbtp": -0.3,
    "preLimiterPeakDbfs": 5.2,
    "limiterGainReductionDb": { "max": 6.3, "mean": 2.1, "pctTimeAbove3": 41 },
    "compressorGainReductionDb": { "max": 1.2, "mean": 0.4, "pctTimeAbove3": 0 },
    "plrDb": 8.8, "crestFactorDb": 9.5, "lra": 2.1,
    "correlation": { "mean": 0.82, "min": 0.4, "monoLossDb": -0.6 },
    "spectrum": { "bands": ["sub<60", "bass60-250", "lowmid250-500", "mid500-2k", "himid2-6k", "air6k+"], "db": [-14, -11, -17, -19, -24, -31] }
  },
  "perBar": [ { "bar": 52, "lufsMomentaryMax": -8.4, "preLimiterPeakDbfs": 4.9, "limiterGrMaxDb": 6.0,
                "topContributors": [ { "id": "channel:acid", "shareOfEnergyPct": 31 } ] } ],
  "elements": [ { "id": "channel:rbass", "name": "Rhythm Bass", "insert": 16, "relativeToMixDb": -20.1,
                  "audibility": { "audibleFractionPct": 6, "maskedBy": [ { "id": "channel:sub", "bandHz": [100, 200], "maskingDb": 14 } ] },
                  "verdict": "inaudible",
                  "suggestions": [ { "why": "…", "patch": [ { "op": "add", "path": "/channels/37/volume", "value": 1.2 } ],
                                     "expectedRelativeToMixDb": -11.5, "expectedAudibleFractionPct": 72 } ] } ],
  "gainReduction": [ { "id": "insert:0/Master", "effect": 1, "type": "limiter", "max": 6.3, "mean": 2.1, "pctTimeAbove3": 41 } ],
  "clashes": [ { "bar": 65, "beatInBar": 0.25, "a": { "channel": "counter", "pattern": "counter-b", "noteIndex": 81, "pitch": "C#5" },
                 "b": { "channel": "pad", "pattern": "pad-C", "noteIndex": 4, "pitch": "C4" }, "interval": "m9", "overlapBeats": 3.7,
                 "severity": "high", "fix": [ … ] } ],
  "findings": [ { "severity": "warn", "rule": "master-overload", "key": "master-overload|bars:52-59", "where": "bars 52–59",
                  "detail": "pre-limiter peaks +5.2 dBFS; …", "fix": [ … ], "fixLabel": "…" } ],
  "render": { "cached": false, "ms": 2380, "prerollBeats": 8, "renders": 1, "sampleRate": 48000 }
}
```

- Ids: `channel:<id>`, `insert:<index>/<name>`; notes by `pattern` and `noteIndex`.
- `verdict`: `inaudible` (audible under 25 % of the time it plays; strict 35,
  loose 15), `buried` (under 60 %), `overloading` (its own peak over 0 dBFS, or
  a big share of a master that overloads), `dominant` (most of the mix), `ok`.
- `suggestions` are JSON Patch against `project.json`, with what the model
  predicts (`expected…`); `--verify` adds `verified`, measured under the patch.
- `whatIf` / `compare`: the master's numbers that moved (`from`, `to`,
  `delta`), the parts whose level, audibility or verdict changed, the findings
  resolved and new, rows that moved 0.5 dB or more, and a one-line `summary`.
- `target`: the delivery target's verdict (`pass`, `warn`, `fail`), the gain
  the platform applies and why. `reference`: the recording's numbers and the
  differences, the spectrum level-matched (the reference moved to the mix's
  loudness, so tone is compared, not loudness).

## Findings and the Critic

Findings are ranked, de-duplicated and at most `--max-findings` (10). Each is a
rule of the Critic (category *Mix check*), with a JSON Patch `fix`:

| rule | when | fix |
|---|---|---|
| `master-overload` | bars where the limiter's input peaks over +1 dBFS and it reduces 3 dB or more (strict: 0 / 2, loose: +3 / 6) | the parts that dominate down; when none does, less limiter drive and every fader down |
| `limiter-pumping` | the master limiter's (or bus compressor's) gain reduction swings 4 dB or more within a beat, over 3 dB a fifth of the time | a slower release, less drive |
| `masked-lead` | the lead (the Critic's role) is buried or inaudible | an EQ cut on its masker where it covers it, or more level |
| `inaudible-part` | a part is inaudible (strict: buried too) | the level the model says it needs |
| `harmonic-clash` | a clash of high severity (strict: medium too) | the quieter note moved to the nearest pitch that clashes with nothing |
| `low-end-buildup` | for 2 bars or more, under 250 Hz is 14 dB over 500 Hz–6 kHz, or 250–500 Hz 6 dB over a balanced tilt | a low shelf on the part (not the bass or drums) carrying the most low end |
| `phase-correlation` | the mix's correlation is negative or it loses 6 dB in mono; a part's correlation under −0.3 | — |
| `section-loudness-flat` | three or more sections all within 1.5 LU (strict 2.5, loose 1.0) | a master volume lane: the sparse sections 1.5–3 dB down |

`rosaclef critic --audio` adds them to the Critic's findings (a whole-song mix
check, cached): `--fix KEY` or `--fix RULE` applies them, `--suppress KEY`
suppresses one, `--disable RULE` turns one off — saved in the project's `critic`
section, which a mix check follows too.

## The studio

The **Mix check** tab of the Maestro panel reads like a mastering meter:

- integrated loudness in large figures, short-term and momentary maxima, and a
  loudness scale with the delivery target's bracket and a PASS / CHECK / FAIL
  chip (the target, as `--target`, is remembered);
- true peak, the peak before the limiter, PLR, LRA, crest factor and mono
  loss, each with a traffic light;
- gain-reduction meters (the limiter, the bus compressor and the busiest
  inserts: the mean as a bar from the top, the peak as a line) and a phase
  correlation meter;
- the loudness history (short-term and momentary, the target, true peaks over
  the limit, the limiter's work hanging from the top; click a bar to show it in
  the playlist) and the spectrum, with a reference track level-matched over it;
- the parts, the most in need first: their verdict, level against the mix and
  audibility; open one for its maskers and suggestions;
- the clashes (click a note to select it in the piano roll) and every row.

**Try** runs a what-if (the song is not touched) and shows what changed;
**Apply** makes the change, one undo step. The report is marked stale when the
song changes; **Measure again** renders it anew (or reads the cache).

## Speed

Measured on a 4-core machine: an 11-bar range of the demo song (a 4-channel
soundfont trio) takes about 1.5 s, the whole song (2:47) about 10 s — the plain
`rosaclef render` of it takes 9 s — and a cached question about 30 ms. Time is
mostly the render: on a 40-channel project with an EQ, a compressor and a reverb
on every insert, a 32-beat range (plus its pre-roll) takes 4.9 s, of which the
engine's rendering is 3.5 s, the measuring 0.8 s and the report 0.3 s. More
cores render faster; the cache answers every further question about the range.

## Where it lives

`crates/studio/src/mixcheck/`: `timeline` (bars, passes, sections, ranges),
`analyze` (the tapped render and the measurements), `cache`, `model`
(loudness, attribution, masking), `clashes`, `report` (the report and its
schema, `mixcheck.schema.json`), `findings` (rules, fixes, suggestions),
`targets`, `reference`, `diff`, `patch` (RFC 6902), `text`, `critic`, `options`
(the request, shared by every interface). The engine's taps, gain-reduction
meters, span seeking and note chasing are in `crates/engine`. The panel is
`web/src/ui/mixcheck.js`.

Tests: `crates/studio/tests/mixcheck.rs` (a golden report of a small project, a
quiet pluck masked by a loud sub, gain reduction of signals of known level,
bars through a meter change and a repeat, what-if, clashes and their fixes),
unit tests of the FFT, K-weighting, true peak and JSON Patch, and
`web/test/mixcheck-smoke.mjs` (the panel in the browser-only studio).
