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
  --verify                                          # re-measure each finding's fix and each suggestion under its patch
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
- Exit status: 0 = no warnings, 1 = warnings (a `warn` finding, or a
  `--target` not met), 2 = an error. Errors name the JSON
  path or the option at fault: `whatIf[0] replace /channels/9/volume: no
  element 9`, or the validation issues of a patched project.
- `project.json` is never written. A what-if is applied in memory.

## The render

One render answers every question: the engine taps every channel (after its
fader), every insert (after its effects and fader), and the master three times
(before its effects, at its limiter's input — after the limiter's own input
gain — and at the output), and the measurements are made as it plays. A
what-if is a second render; `--verify`, one per finding fix and suggestion
(at most 10).

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
- The render uses the engine `rosaclef render` uses, on every core; the
  measurements of each chunk are made on every core too. A range starts a new
  engine at its pre-roll, so instruments with randomness (drift, shimmer,
  generative parts) are not sample-identical to a whole-song render there —
  the measured levels agree within a fraction of a dB (a test checks it). The
  numbers are deterministic: the same song and range give the same report.
- The key does not cover the installed soundfonts or a plugin's binary (both
  change with the installation, not the song): after updating them, pass
  `--no-cache` (or `"cache": false`).
- One mix check runs at a time (each render takes every core); `--verify`
  re-measures at most 10 suggestions.

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
  15 % of it — at every threshold: `--threshold` moves the verdicts' cut-offs,
  not the measure; frames too quiet to hear even alone (the end of a decay) do
  not count. `audibleFractionPct` is the share of the
  part's frames that are audible; `maskedBy` names the parts with the strongest
  excitation where it is loudest, the band and by how much.
- **Clashes** — notes of two parts sounding together a minor second (or minor
  ninth…), a major seventh or a tritone apart, from the notes as they play and
  the parts' measured levels at that moment, weighed by overlap × the quieter
  part's level. Short passing tones are left out unless `--threshold strict`.
  Each names both notes (`pattern`, `noteIndex`, pitch) and the same two notes
  clashing again in a loop are one clash (`alsoInBars`). The chord sounding
  with them (every note, read above the lowest) can make the interval a
  colour, not a mistake: the tensions of a dominant seventh (♭9, ♯9, ♯11, ♭13,
  13), a major seventh chord's seventh and ♯11, a lydian ♯11 over a major
  chord, a diminished chord's tritone — and a passing or approach note (by
  step, a beat or less), or a suspension (held over from the chord before, on
  which it belonged, and moving on by step). Those rank low and say why
  (`idiom`). A wrong note ranks high and says why (`outOfChord`): the other
  parts play a plain major or minor triad, the note is not in it but a
  semitone from one of its notes that two parts or more sound, lasts 1.5 beats
  or more and resolves nowhere (extended and altered chords are left alone);
  one finding per wrong note, however many parts it rubs against. Clashes are
  reported, never fixed: the notes are the song's, and a mix check's fixes
  touch the mixer only (faders, EQ, dynamics, the master's automation).

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
                 "severity": "high" } ],
  "findings": [ { "severity": "warn", "rule": "master-overload", "key": "master-overload|master", "where": "bars 52–59",
                  "detail": "pre-limiter peaks +5.2 dBFS (its input gain alone adds +6.0 dB); …", "fix": [ … ],
                  "fixLabel": "the limiter's input gain +6.0 → +0.8 dB",
                  "verified": { "resolved": true, "summary": "loudness -3.1 LU; pre-limiter peak -5.2 dB; 1 finding(s) resolved", "new": [] } } ],
  "render": { "cached": false, "ms": 2380, "prerollBeats": 8, "renders": 1, "sampleRate": 48000 }
}
```

- Ids: `channel:<id>`, `insert:<index>/<name>`; notes by `pattern` and `noteIndex`.
- `verdict`: `inaudible` (audible under 25 % of the time it plays; strict 35,
  loose 15), `buried` (any part audible under 60 % of the time; the lead also
  when it sits more than 10 LU under the mix however audible — strict 8, loose
  13 — over the range, or in stretches covering a fifth of where it plays:
  sections, or 4 bars without them, each pass apart, listed in `buriedIn`),
  `overloading` (its own peak over 0 dBFS, or a big share of a master that
  overloads), `dominant` (most of the mix), `ok`. The lead carries
  `"lead": true` (one per song). A part playing under 5 % of the range (a
  release tail) gets no audibility finding.
- `suggestions` are JSON Patch against `project.json`, with what the model
  predicts (`expected…`). A lead under the mix gets its balance first (over
  the stretches where it is under, when that is where): an EQ boost of 6 dB
  or more on a part covering it, in that range, back to +2 dB; the faders of
  the parts over it that sit above unity back to unity; the lead up (a cut
  insert fader restored first), 6 dB at most, and the parts over it down for
  the rest, to about -5 LU against the mix. An EQ suggestion says what it
  does to the EQ there (a band in use near the frequency is moved, not a new
  one added).
- `--verify` measures every finding's `fix` (the whole check again with it
  applied: `verified.resolved`, `summary`, `new` findings, and `still` — what
  and where — when not resolved), then the suggestions (`verified`: the part's
  level, audibility and verdict), one render each, at most 10. The summary
  gives the dynamics that moved (LRA, PLR, the master compressor's and
  limiter's mean reduction), so "resolved" can be weighed; `--text` shows
  both and names the new findings.
- Finding keys are stable (`master-overload|master`, `masked-lead|channel:sax`,
  `harmonic-clash|<pattern>:<note>|<pattern>:<note>`): the same problem keeps
  its key when it moves or shrinks, so `whatIf`/`compare` list it as resolved
  only when it is gone.
- `whatIf` / `compare`: the master's numbers that moved (`from`, `to`,
  `delta`), the parts whose level, audibility or verdict changed, the findings
  resolved and new, rows that moved 0.5 dB or more, and a one-line `summary`.
- `target`: the delivery target's verdict (`pass`, `warn`, `fail`), the gain
  the platform applies and why. `reference`: the recording's numbers and the
  differences, the spectrum level-matched (the reference moved to the mix's
  loudness, so tone is compared, not loudness). Over HTTP the reference must
  be a file in the project folder (at most 512 MB); the command line takes any
  file.
- A report made with `--what-if` is the report of the *patched* project: its
  fixes and suggestions point into that project (`whatIf.note` says so). Apply
  the what-if first, or measure again without it, before applying them.

## Findings and the Critic

Findings are ranked, de-duplicated and at most `--max-findings` (10), and no
rule takes more than a third of the list. One problem is one finding, with one
fix: an overload or a low-end build-up lists every stretch it happens in
("bars 1–7, 9–12 (pass 2), 13–18"; more than four read "bars 2–140 (112 bars,
in 20 stretches)"). Every fix is a change to the mixer — faders, EQ, dynamics,
the master's automation — never to the notes. Fixes go to the setting at fault
when there is one — a limiter's drive, an EQ's boost, a fader pushed above
unity — rather than turning everything else down. Each is a rule of the Critic
(category *Mix check*), with a JSON Patch `fix` where the mixer can fix it:

| rule | when | fix |
|---|---|---|
| `master-overload` | bars where the limiter's input peaks over +1 dBFS and it reduces 6 dB or more (strict: 0 / 3, loose: +3 / 9) — catching the odd peak is mastering, not overload | a limiter driven more than 3 dB: its drive first; then the parts carrying 15 % or more of the mix down (up to three); when none does, the rest of the drive and every fader down |
| `true-peak` | the output's true peak over 0 dBTP (strict −1, loose +0.5): peaks between the samples clip when converted or encoded | the limiter's ceiling down to land at −1 dBTP (no limiter: the master fader) |
| `limiter-pumping` | the master limiter's (or bus compressor's) gain reduction swings 4 dB or more within a beat, over 3 dB a fifth of the time | a slower release (at least 60 ms for a limiter, 150 ms for a compressor); less drive, until the limiter takes 3 dB at most (a compressor: a higher threshold) |
| `over-compression` | a master compressor or limiter takes 6 dB or more on average (strict 4, loose 9), over 3 dB at least half the time: the mix is squashed flat | down to about 3 dB of gain reduction: a compressor glues instead (ratio 2.5:1 at most, attack at least 10 ms, release 150 ms when under 100), its threshold where it takes about 3 dB and its makeup down by as much (the loudness kept); a limiter's drive down |
| `masked-lead` | the lead is buried or inaudible, or more than 10 LU under the mix (strict 8, loose 13) over the range or in stretches (`buriedIn`; the finding names their bars). The lead is the part named like one (lead, vocal, melody, topline, solo), else the loudest the Critic reads as a lead: one per song | its balance (a masker's EQ boost over it back, boosted faders over it back to unity, the lead up), else an EQ cut on its masker where it covers it, or more level |
| `inaudible-part` | a part is inaudible (strict: buried too), playing 5 % of the range or more; a channel volume 12 dB or more under the others' is named | the level the model says it needs — only when the model says it helps and the faders can reach it (else the finding says so); more than 12 dB only when its own fader is that far under the others' |
| `harmonic-clash` | a clash of high severity (strict: medium too). Minor seconds and ninths rank highest; a major seventh and a tritone one step lower; notes more than two octaves apart are not clashes; a colour of the chord, a passing note or a suspension ranks low; a wrong note (`outOfChord`) ranks high | — (the notes are the song's: the finding names both, by `pattern` and `noteIndex`) |
| `low-end-buildup` | a part carrying a quarter of the lows boosts them 6 dB or more with an EQ (strict 4, loose 9) in a mix whose lows lean 3 dB over (strict 2, loose 5); else for 2 bars or more, under 250 Hz is 14 dB over 500 Hz–6 kHz, or 250–500 Hz 6 dB over a balanced tilt (drum breaks aside) | that EQ's boost down to +3 dB or less; else, on the part boosting its lows or (not the bass or drums) carrying the most low end, a low-shelf cut at 150 Hz — or a bell cut at 350 Hz when the excess is in the low mids |
| `phase-correlation` | the mix's correlation is negative or it loses 6 dB in mono; a part's correlation under −0.3 (however quiet: it vanishes in mono) | — |
| `section-loudness-flat` | three or more sections all within 1.5 LU (strict 2.5, loose 1.0) | a master volume lane: the sparse sections 1.5–3 dB down |

`rosaclef critic --audio` adds them to the Critic's findings (a whole-song mix
check, cached): `--fix KEY` or `--fix RULE` applies them (each was measured on
the song as it was, so a fix that touches what an earlier one in the same run
changed is skipped until the next measurement), `--suppress KEY`
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
song changes, and its Try and Apply wait for **Measure again** (its fixes point
at notes and devices by position).

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

Tests:

- `crates/studio/tests/mixcheck.rs`: a golden report of a small project
  (checked against `mixcheck.schema.json`), a quiet pluck masked by a loud sub,
  gain reduction of signals of known level, bars through a meter change and a
  repeat, a range measured like the same bars of the whole song (a repeat's
  second pass too), the cache in memory and on disk, what-if, clashes reported
  with their notes and no fix touching them, and bad requests (ranges, a reference outside the folder, a fix to a
  song read only in part) refused by name.
- Unit tests: the FFT, K-weighting, true peak, loudness and loudness range of
  signals of known loudness (after EBU Tech 3341 / 3342), JSON Patch.
- "Trouble" (`tests/mixcheck/trouble.json`), a song made with faults on
  purpose — a driven limiter with a 5 ms release under hot faders, a lead
  under a pad in its register, a rhythm bass under the sub, a counter-melody's
  minor ninth over the pad, warm keys thickening the low mids, a "widener"
  whose channels cancel, four sections at one loudness, a repeated chorus and
  a 3/4 outro: every fault is found, by the right rule, on the right part, and
  applying the fixes round after round (as an agent would) brings the limiter
  to rest and the lead through.
- `crates/server/tests/mixcheck_cli.rs`: the command line's exit status and
  errors, and that a what-if never writes the project.
- `web/test/mixcheck-smoke.mjs`: the panel in the browser-only studio —
  measure, try, apply (the note moves), a stale report refused, undo.
