# Mix check

`rosaclef mixcheck` answers mix questions with numbers, from **one render**:
*is the rhythm bass audible here? is section C overloading the master? is the
lead masked in the intro? what is the limiter doing at bar 94?* It replaces copying the project to cut bars out, stripping
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
  --checks levels,audibility,masking,dynamics,gainreduction,spectrum,stereo
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
  bands (Schroeder's spreading function, a fixed 10 dB masking index) into
  excitations, and a loudness increment after Zwicker and Moore–Glasberg
  (simplified: no tonality, temporal masking or binaural unmasking) compares
  what the part adds to the masker's loudness with its loudness alone. A frame
  is audible when it keeps at least 15 % of its loudness (sones) — at every
  threshold: `--threshold` moves the verdicts' cut-offs, not the measure;
  frames too quiet to hear even alone (the end of a decay) do not count.
  `audibleFractionPct` is the share of it that is audible, its frames weighted
  by its own loudness (its decays and tails, masked by nature, count little).
  It is a prominence heuristic, not a detection threshold: a part read as
  buried may still be felt. `maskedBy` names the parts with the strongest
  excitation where it is loudest, the band and the excitation ratio.
- **The limiter** on the master looks ahead 1.5 ms and holds the gain each
  stretch between two samples needs over that window — the two samples and the
  signal between them, oversampled 4× as BS.1770 measures true peak — so
  neither a sample nor a peak between samples passes its ceiling (a true-peak
  limiter: the ceiling is the true peak). Its metered gain reduction is what
  it applies.
- **Roles.** Besides the lead, the kick and the bass are **anchors** (a drum
  channel named for a kick — kick, bd, bass drum —, and the bass the Critic
  reads; not a layer). They are judged by their level against the mix too —
  audible is not balanced — against a range set by the style: dance music (a
  kick on most beats) keeps the kick at −9 to −6 dB and the bass at −13 to
  −10; other music −15 to −6 (`rangeDb`). The other findings' fixes hold the
  anchors where they are and never cut one to make room for a supporting part.
- **Silenced on purpose.** A channel muted, or at volume 0 (under −60 dB) with
  no lane bringing it up, and a muted insert, are left out: no finding is about
  them and no fix touches them.
- **Notes are not its business.** Harmonic clashes are read from the notes
  by the Critic (`rosaclef critic`, its harmony checks); a mix check measures
  the sound, and every fix it proposes changes the mixer only — faders, EQ,
  dynamics, automation — never a note.

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
  "findings": [ { "severity": "warn", "rule": "master-overload", "key": "master-overload|master", "where": "bars 52–59",
                  "detail": "pre-limiter peaks +5.2 dBFS (its input gain alone adds +6.0 dB); …", "fix": [ … ],
                  "fixLabel": "the limiter's input gain +6.0 → +0.8 dB",
                  "verified": { "resolved": true, "summary": "loudness -3.1 LU; pre-limiter peak -5.2 dB; 1 finding(s) resolved", "new": [] } } ],
  "render": { "cached": false, "ms": 2380, "prerollBeats": 8, "renders": 1, "sampleRate": 48000 }
}
```

- Ids: `channel:<id>`, `insert:<index>/<name>`; notes by `pattern` and `noteIndex`.
- `anchor` (`kick`, `bass`) and `rangeDb`: an anchor and where it belongs.
- `verdict`: `weak` — an anchor under its range by half a dB, over the range
  or in a quarter of its stretches, or by a dB in any, however audible;
  `inaudible` (audible under 25 % of the time it plays; strict 35,
  loose 15; a transition — a riser, an impact — judged on its attack: the
  frames within 6 dB of its peak over the second before, not its tail),
  `buried` (any part audible under 60 % of the time; the lead also
  when it sits more than 10 LU under the mix however audible — strict 8, loose
  13 — over the range, or in stretches covering a fifth of where it plays:
  sections, or 4 bars without them, each pass apart, listed in `buriedIn`),
  `overloading` (40 % or more of the mix in a song whose master overloads:
  the limiter's input over 0 dBFS in a bar where it takes 3 dB or more — a
  part's own peak over 0 dBFS clips nothing in the engine), `dominant` (45 % or more of the mix, within 3 LU of
  it), `ok`. The lead carries
  `"lead": true` (one per song). A part playing under 5 % of the range (a
  release tail) gets no audibility finding. With `spectrum` checked, each part
  has `spectrumDb`: its level in the six master bands where it plays.
- `bySection`: each part's level against the mix per section (per 4 bars
  without sections), each pass apart, where it plays 2 s or more; `--by
  section --text` prints it as a table. A part other than the lead that
  falls 10 dB under its own loudest stretch (strict 8, loose 13) and 15 dB
  under the mix drops out there: `buriedIn` lists those stretches.
- `role` is the Critic's; one part per song is `lead`, another the Critic
  reads as a lead is `melody`.
- A level fix on a fader an automation lane drives moves the lane's points
  instead (the fader does nothing while the lane plays).
- `--text` lists the parts that are fine on one line (level against the mix,
  audible %), and notes (`note: range: the song ends at bar 40; measured
  38–40`) when a range was cut to fit the song.
- A part is heard after its insert when it is alone on it (its effects and
  fader included), else at its channel. `balanceDb` is its left against its
  right (a hard-panned part has no correlation: one side is silent);
  `insertEffectsDb` is how much its insert's effects change its level, and
  `wetPct` the share of what is heard of it that its time effects add (a
  reverb's or chorus's dry signal falls as their mix rises; a delay's does
  not).
- RMS and crest factor count the hops with sound only (an intro's rests
  would inflate them); a sine reads −3 dBFS RMS.
- `perBar` rows measure their loudness from the windows that start inside the
  row (a momentary window is 400 ms, a short-term one 3 s), so a quiet bar
  after a loud one reads quiet.
- `--checks` limits the measurement and the findings alike: without `levels`
  no overload, true-peak, section or level findings; without `spectrum` no
  low-end or high-end findings; without `gainreduction` no pumping or
  over-compression; without `stereo` no phase findings; without
  `audibility`, `masking` and `levels` no part findings.
- `suggestions` are JSON Patch against `project.json`, with what the model
  predicts (`expected…`). A lead under the mix gets its balance first (over
  the stretches where it is under, when that is where): an automation lane on
  its volume holding it 6 dB or more down there (and up elsewhere) back up;
  an EQ boost of 6 dB
  or more on a part covering it, in that range, back to +2 dB; the faders of
  the parts over it that sit above unity back to unity; the lead up (a cut
  insert fader restored first), 6 dB at most, and the parts over it down for
  the rest, to about -5 LU against the mix. An EQ suggestion says what it
  does to the EQ there (a band in use near the frequency is moved, not a new
  one added).
- `--verify` measures every finding's `fix` (the whole check again with it
  applied: `verified.resolved`, `summary`, `new` findings, `worse` — findings
  already there that grew under it — `levelMatchDb` when it changes the
  loudness by 1 dB or more (play it that much louder to compare fairly),
  `sideEffects` — what it did to the other parts against the mix: the anchors
  and the lead when they move 0.3 dB or more, any part at 1 dB —, and
  `still` — what and where — when not resolved), then the suggestions (`verified`: the part's
  level, audibility and verdict), one render each, at most 10. The summary
  gives the dynamics that moved (LRA, PLR, the master compressor's and
  limiter's mean reduction), so "resolved" can be weighed; `--text` shows
  both and names the new findings.
- Finding keys are stable (`master-overload|master`, `masked-lead|channel:sax`):
  the same problem keeps
  its key when it moves or shrinks, so `whatIf`/`compare` list it as resolved
  only when it is gone.
- `whatIf` / `compare`: the master's numbers that moved (`from`, `to`,
  `delta`), the parts whose level, audibility or verdict changed, the findings
  resolved and new, rows that moved 0.5 dB or more, and a one-line `summary`.
  A what-if op setting a value an automation lane drives (a fader with a
  volume lane) does nothing while the lane plays: `whatIf.overridden` names
  it, and the lane's points are what to change.
- `target`: the delivery target's verdict (`pass`, `warn`, `fail`), the gain
  the platform applies (`playbackGainDb`) and why. Loud masters are turned
  down; quiet ones up only as far as the platform allows — Spotify and Apple
  until the true peak reaches −1 dBTP, YouTube, Amazon and Tidal not at all —
  and a quiet master is a warning only when it ends up more than 6 dB under
  the rest (a dynamic jazz or classical master is a choice). Spotify asks −2 dBTP of a master louder than
  −14 LUFS (−1 otherwise); the verdict and the `true-peak` finding follow it. `reference`: the recording's numbers and the
  differences, the spectrum level-matched (the reference moved to the mix's
  loudness, so tone is compared, not loudness). Over HTTP the reference must
  be a file in the project folder (at most 512 MB); the command line takes any
  file.
- A report made with `--what-if` is the report of the *patched* project: its
  fixes and suggestions point into that project (`whatIf.note` says so). Apply
  the what-if first, or measure again without it, before applying them.

## Findings and the Critic

Findings are ranked — warnings first, then in a mix engineer's order: the
balance and what covers what, tone, stereo, the dynamics of parts and bus,
the limiter's drive and release, the sections' shape, and the ceiling last —
de-duplicated and at most `--max-findings` (10), and no
rule takes more than a third of the list. One problem is one finding, with one
fix: an overload or a low-end build-up lists every stretch it happens in
("bars 1–7, 9–12 (pass 2), 13–18"; more than four read "bars 2–140 (112 bars,
in 20 stretches)"). Every fix is a change to the mixer — faders, EQ, dynamics,
the master's automation — never to the notes. A fix moves one level of the
gain chain: a part's insert fader when the insert carries it alone (up to 2),
else its channel volume, never raised past unity; a lane driving that fader
moves instead, and is never pushed past unity by a fix. Taken together the
fixes keep the anchors in their ranges — the boosts are scaled back as one
when they would not — and each says what it does to the anchors and the lead
against the mix (`sideEffects`, the model's prediction; `--verify` measures
them). Fixes go to the setting at fault
when there is one — a limiter's drive, an EQ's boost, a fader pushed above
unity — rather than turning everything else down. Each is a rule of the Critic
(category *Mix check*), with a JSON Patch `fix` where the mixer can fix it:

| rule | when | fix |
|---|---|---|
| `weak-anchor` | the kick or the bass under its range against the mix (`rangeDb`) by half a dB, over the range or in a quarter of its stretches — heard, and weak: the groove loses its weight. The stretches under the range are named | one fix for all the weak anchors, solved together: each up at its level of the gain chain until its stretches sit in its range (its weakest at the floor; spread wider than the range, centred on it), the other anchors up with what that adds to the mix, and the limiter's input down by as much — the loudness and the other anchors' places kept; sections one fader cannot place, a lane on its insert (`<id>-level`) holding each in the range (weak ones up, hot ones down) |
| `masked-lead` | the lead is buried or inaudible, or more than 10 LU under the mix (strict 8, loose 13) over the range or in stretches (`buriedIn`; the finding names their bars). The lead is the part named like one (lead, vocal, melody, topline, solo — not backing, chop, choir, harmony, double, ad-lib), else the loudest the Critic reads as a lead: one per song | its balance: an automation lane holding it down put back, a masker's EQ boost over it back to +2 dB, pushed faders back to unity, the lead up (6 dB at most), and only the parts covering it down, 4 dB at most; else an EQ cut on its masker where it covers it |
| `inaudible-part` | a part is inaudible (strict: buried too), playing 5 % of the range or more; a channel volume 12 dB or more under the others' is named; notes much softer or sparser here than elsewhere in the song are named. A layer covered by the part it layers is info, with no fix (it is heard as that part) | the first of, in this order: an automation lane lifting what covers it over unity, back to unity there (the cause); under the kick and the bass below 300 Hz, a high-pass on it (120–150 Hz) leaving the lows to them; a cut in what covers it, never an anchor; and only then its level — the level the model says it needs, 6 dB at most (more only when its own fader sits that far under the others'); beyond that the finding calls it an arrangement conflict |
| `part-dropout` | a part (not the lead) whose own level falls 10 dB (strict 8, loose 13) under its loudest stretch and sits 15 dB under the mix there; warn when a volume lane holds it down and it is gone (25 dB under the mix), else info; the notes are named when they are softer or sparser there (pattern, velocity) | the lane back up there |
| `dominant-part` | a part is 60 % or more of the mix's loudness (strict 50, loose 75) — the lead 80 % (strict 70, loose 90): the band behind it disappears; warn when its faders sit 3 dB or more over unity, else info | its faders back to unity |
| `low-end-buildup` | a part carrying a quarter of the lows boosts them 6 dB or more with an EQ (strict 4, loose 9) in a mix that itself leans low (lows within 4 dB of the rule's threshold, or the low mids within 2); else for 2 bars or more, under 250 Hz is 14 dB over 500 Hz–6 kHz, or 250–500 Hz 6 dB over a balanced tilt (drum breaks and a part playing alone aside) | that EQ's boost down to +3 dB or less; a low-mid bell boost back to +2 dB; else, on the part carrying the most low end (not the bass or drums), a low-shelf cut at 150 Hz — or a bell cut at 350 Hz when the excess is in the low mids |
| `boxy-lowmids` | 250–500 Hz within 3 dB of 500 Hz–2 kHz (strict 4, loose 1) with a part carrying a quarter of it boosting it 6 dB or more (a bell at 200–600 Hz, a low shelf at 250 Hz or more); or, with no such boost, 250–500 Hz 2 dB over 500 Hz–2 kHz (strict 1, loose 4) | that boost back to +2 dB; else a −3 dB bell at 350 Hz on the part carrying the most of it (not the bass or drums) |
| `harsh-presence` | 2–6 kHz 2 dB over 500 Hz–2 kHz (strict 1, loose 4) — distorted guitars make a mix that bright; or within 1 dB of it (strict 2, loose over by 1) with a part carrying a tenth of it boosting it 6 dB or more: harsh, fatiguing | a part carrying a quarter of it boosting it 6 dB or more (a bell at 2–6 kHz, a high shelf from 4 kHz down): back to +2 dB; else a −3 dB bell at 3.5 kHz on the part carrying the most of it, not the lead when another can give |
| `bright-highs` | above 6 kHz within 2 dB of 2–6 kHz (strict 3, loose 0): bright, sizzly (often a choice in airy pop or EDM) | a high boost on a part carrying a quarter of it back to +2 dB; else a −3 dB high shelf at 8 kHz on the part carrying the most highs |
| `reverb-wash` | half or more of what is heard of a part is its insert's reverb, delay, chorus or phaser (`wetPct`; strict 40, loose 65), or a fifth with a delay feeding back 0.7 or more (the repeats pile up): far away, washy, smearing into the rest | the reverb's mix down to where it is about a quarter; a delay's feedback to 0.35 and its mix to 0.25 |
| `low-end-off-centre` | a part with half or more of it under 250 Hz is 6 dB louder on one side (strict 4, loose 10) | its pan back to the centre |
| `lr-balance` (info) | the mix is 1.5 dB louder on one side (strict 1, loose 3), no low-end part explaining it | — (the parts panned that way are named) |
| `phase-correlation` | the mix's correlation is negative or it loses 6 dB in mono; a part's correlation under −0.3 (however quiet: it vanishes in mono) | — (the mixer has no polarity switch); the finding names the cause: its insert's stereo effects, else the audio files it plays (one channel out of polarity) |
| `part-over-compression` | a compressor on a part takes 8 dB on average where it plays (strict 6, loose 12), or 15 dB at its peaks with an attack under 2 ms | about 3 dB of reduction, 4:1 at most, attack 10 ms, release 150 ms, the makeup down by as much (the loudness kept) |
| `over-compression` | a master compressor or limiter takes 6 dB or more on average (strict 4, loose 9), over 3 dB at least half the time: the mix is squashed flat | down to about 3 dB of gain reduction: a compressor glues instead (ratio 2.5:1 at most, attack at least 10 ms, release 150 ms when under 100), its threshold where it takes about 3 dB and its makeup down by as much (the loudness kept); a limiter's drive down |
| `master-overload` | bars where the limiter's input peaks over +1 dBFS and it reduces 8 dB or more (strict: 0 / 5, loose: +3 / 11) — loud genres run a limiter 6 dB deep on their peaks; catching the odd peak is mastering, not overload | a limiter driven more than 3 dB: its drive first; then the parts carrying 15 % or more of the mix down (up to three; not the anchors); when none does, the rest of the drive and every fader down |
| `squashed-kick` | bars where the master limiter takes 3 dB or more (strict 2, loose 5) off the kick's hits on average, 1 dB more than off the rest of the bar: its transients flattened, the drums without punch | the limiter's input gain down until it takes about 2 dB off the hits (1 to 6 dB) — less going in, not a lower ceiling |
| `limiter-pumping` | the master limiter's (or bus compressor's) gain reduction swings 4 dB or more within a beat, over 3 dB a fifth of the time | less drive, until the limiter takes 3 dB at most (a compressor: a higher threshold); a release that holds steady instead of recovering between hits: about half a beat for a limiter (100–300 ms), a beat for a compressor (150–600 ms) |
| `fast-limiter-release` | the master limiter releases in under 20 ms and takes 3 dB or more: it rides the bass's waveform and distorts the lows | release about half a beat (100–300 ms) |
| `section-lift` (info) | a chorus (a section named chorus, drop, hook or refrain) less than 1 LU (strict 0.5, loose 2) louder than the verses' average — a heuristic: a chorus often lifts by density and width; not judged while `master-overload` or `over-compression` fires (the limiter clamps everything) | a master volume lane holding it 1.5 dB or more down: back up there, and the rest of the song down on it (3 dB at most) — never the choruses over the lane's level elsewhere (the master fader is after the limiter); else what in the mixer holds it back, named: lanes holding parts 3 dB or more down there (put back), the master limiter taking 1.5 dB more there than in the verses (its drive down by as much), a part that is most of the mix's loudness; else none (the arrangement has to lift it) |
| `section-loudness-flat` (info) | three or more sections all within 1.5 LU (strict 2.5, loose 1.0); not judged while the limiter clamps everything | a master volume lane: the sparse sections 1.5–3 dB down |
| `loud-master` (info) | no `--target`, and the master louder than −10 LUFS (strict −12, loose −8): streaming services turn it down (most to −14, Apple to −16) | the limiter's drive down to land near −12 |
| `gain-staging` (info) | channels over unity on an insert whose fader is under unity: the gain at the wrong end of the chain | the insert's fader up (to unity at most) and its channels down by as much (lanes on them too): the same sound |
| `master-fader` | the master fader, which comes after the limiter, is off unity: down 1 dB or more (info: the ceiling no longer sets the peaks), or up (warn: it pushes them over) | the fader to 0 dB and the limiter's input gain by as much (the same loudness; the input gain goes to −24 dB) |
| `true-peak` | the output's true peak over the delivery target's limit (`--target`), else −1 dBTP (strict −2, loose 0) — ranked last: the drive and release come first. Info, its fix held back, while other fixes in the report raise faders (they drive the limiter and the true peak up: measure again after them). Also info when the ceiling sits lower than the limit needs (the true peak a dB or more under it) | the smallest move that reaches the limit: the ceiling down by as much as the true peak is over it, and a tenth (no limiter: the master fader); a limiter taking 6 dB or more: its input gain down first. A ceiling lower than needed: up to where the true peak meets the limit, a few tenths under |

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
- every row.

A progress bar runs while it measures — how far into the range the render
has come, then the measuring; Export shows the same. Both run as jobs: `POST
/api/jobs/mixcheck` (the body of `POST /api/mixcheck`) or `POST
/api/jobs/render` answers at once with `{"job": ID}`, and `GET /api/jobs/ID`
says how far it has come (`{id, what, stage, done, seconds, total, render,
state: "running"}`) until `state` is `"done"` with its `result` (what the
request on its own answers) or `"failed"` with its `error`. Several jobs at
once each answer for themselves; in the browser-only studio the page serves the
same exchange, its worker telling it as the job goes. With no report yet, an
arrow points at **Measure**.

Each finding with a fix has:

- a box to include it in **Quick fix**, ticked by default;
- **Show** (a finding about some bars): those bars in the score or the
  playlist, whichever is open, with the playhead there;
- **Before / after**: from the playhead, 4 bars as the song is, a click, then
  the same 4 bars with the fix — nothing is changed;
- **Apply fix**: the change, one undo step.

**Quick fix**, above the findings, does the same with every ticked fix at once
(where two set the same thing, the higher-ranked wins): Before / after, Apply
all. Before / after is how to judge a fix before making it — it plays, it does
not render; the numbers come from **Measure again** once it is applied (Ctrl+Z
takes it back). After **Measure again**, a line compares it with the measure
before (integrated loudness, true peak, PLR, the findings gone and new), and
**Previous** shows that report whole until **Latest**. After a fix is applied, the report's other fixes still apply
without measuring again — unless they set what an applied one set, or the song
changed some other way (then they wait for **Measure again**: they point at
notes and devices by position).

## Speed

Measured on a 4-core machine: an 11-bar range of the demo song (a 4-channel
soundfont trio) takes about 1.5 s, the whole song (2:47) about 10 s — the plain
`rosaclef render` of it takes 9 s — and a cached question about 30 ms. Time is
mostly the render: on a 40-channel project with an EQ, a compressor and a reverb
on every insert, a 32-beat range (plus its pre-roll) takes 4.9 s, of which the
engine's rendering is 3.5 s, the measuring 0.8 s and the report 0.3 s. More
cores render faster; the cache answers every further question about the range.
A change to the mixer alone (a fader, an effect, a channel's volume or pan — a
fix applied) plays the instruments' outputs kept from the last render instead
of running the instruments again: the demo song measures again in about 10 s
instead of 19, the same report to the last digit. The outputs are kept on disk
only, never in memory — `.rosaclef/mixcheck/dry-<key>/`, written as the render
goes and read as the next one plays (about 210 MB for the demo song; silence
takes none) — so they outlast a restart; the newest two renders' are kept. The
browser-only studio has no disk for them and renders in full.

## Where it lives

`crates/studio/src/mixcheck/`: `timeline` (bars, passes, sections, ranges),
`analyze` (the tapped render and the measurements), `cache`, `model`
(loudness, attribution, masking), `report` (the report and its
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
  second pass too), the cache in memory and on disk, what-if, fixes touching
  the mixer only, and bad requests (ranges, a reference outside the folder, a fix to a
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
  measure, try, apply (the limiter's drive), a stale report refused, undo.
