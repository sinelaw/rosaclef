# The Critic

The Critic lints the song against the rules of thumb of composition,
arrangement, sound design and mixing. The checks are mechanical: no AI and no
audio analysis. They read `project.json` (notes, clips, channels, the mixer,
automation) and nothing else, so the same song always gets the same findings.
(The *Mix check* category is the exception: it is measured on a render — see
[Audio checks](#audio-checks-mix-check).)

- **Suggestions** come with a fix: a list of JSON Patch operations
  ([RFC 6902](https://www.rfc-editor.org/rfc/rfc6902) `add` and `remove`) on
  `project.json`.
- **Issues** are for information only.
- **Off by default:** the checks of classical theory (keys, counterpoint,
  singable melodies) don't run unless a project turns them on. Most modern
  tracks break those rules on purpose, so they would mostly be noise. They are
  marked *off by default* in the tables below.
- **Suppressing** hides one finding (by its key), or turns a whole check off
  (by its rule id). Turning a check on, or off, and suppressing are saved in
  the project, in a `critic` section, like a linter's configuration. Only the
  departures from the defaults are written:

  ```json
  "critic": { "off": ["loopitis"], "on": ["parallel-fifths"], "suppress": ["no-ghost-notes|pattern:drums-head:drums:-1"] }
  ```

  The studio, the command line and the agent all leave out the same things.
  A suppressed finding is still reported, marked as suppressed, so it can be
  brought back.

## Where it runs

The checks are written in Rust, in [`crates/core/src/critic`](../crates/core/src/critic),
next to validation:

- `analysis.rs`: channel roles, the song as it plays, bars, chords, and
  helpers to word findings and build fixes.
- `harmony.rs`: the key, voicings, voice leading, clashes, melody and
  instrument ranges.
- `rhythm.rs`: velocities, timing and MIDI hygiene.
- `arrangement.rs`: the playlist, repetition, density over time, and project
  hygiene.
- `mixing.rs`: the low end, the mixer, stereo, effect chains and the master.

Everything reaches the checks through one core:

- **In the studio:** the Critic tab in the Maestro panel. The panel
  ([`web/src/ui/critic.js`](../web/src/ui/critic.js)) posts the song to
  `POST /api/critic` 350 ms after an edit, and never during a drag. The
  native server and the browser-only studio's worker both answer it.
  - **Apply** sends `fix` (finding keys or rule ids) and gets the fixed song
    back. It is one undo step, and nothing is applied if the song changed in
    the meantime.
  - **Suppress** (×), **Turn off** (on a check's heading) and the checks list
    edit the project's `critic` section. Ctrl+Z undoes them.
  - **Ask Maestro** types an issue into the agent's prompt.
  - **Show** selects the notes in the piano roll, opens the insert in the
    mixer, or scrolls to the bar in the playlist.
- **On the command line:**

  ```sh
  rosaclef critic [DIR]                       # the findings, grouped, with their keys
  rosaclef critic --json                      # the same, with each fix's operations
  rosaclef critic --fix KEY|RULE|all          # apply fixes and save (repeatable)
  rosaclef critic --suppress KEY|RULE         # suppress a finding, or turn a check off
  rosaclef critic --unsuppress KEY|RULE       # bring it back
  rosaclef critic --enable RULE               # turn a check on (e.g. a theory check)
  rosaclef critic --disable RULE              # turn a check off
  rosaclef critic --suppressed                # list the suppressed findings too
  rosaclef critic --rules                     # the checks, which are on, and which start off
  ```

  The browser-only studio's shell has `critic`, `critic enable|disable RULE`, `critic fix|suppress|unsuppress
  KEY|RULE` and `critic rules`. `AGENTS.md` asks the agent to run
  `rosaclef critic` after its edits, and to leave suppressed findings alone.

`--fix` applies one finding after another, and looks each up again on the
song as the fixes before it left it, since a fix can move the notes another
points at. A fix that would leave the project invalid is refused.

## Audio checks (Mix check)

The checks above read the project only. The *Mix check* category is measured on
a render: master overload, true peaks over 0 dBTP, limiter pumping, a masked
or buried lead, inaudible parts,
harmonic clashes weighed by the parts' real levels, low-end build-up, phase,
and sections without a build. `rosaclef critic --audio` runs them (one render
of the song, cached) and lists, fixes and suppresses them like the others. They
come from `rosaclef mixcheck`; see [`mixcheck.md`](mixcheck.md).

## Content this version doesn't know

A project made with a newer Rosaclef may hold sections, fields, instruments,
effects or settings that this version doesn't know.
[`crates/core/src/compat.rs`](../crates/core/src/compat.rs) loads it to play
anyway, driven by the JSON schema:

- Unknown sections and fields are ignored.
- An unknown instrument plays on a stand-in (the analog synth).
- An unknown effect is bypassed.
- Unknown parameters and option values fall back to their defaults.

The audio engine (WebAssembly) and `rosaclef render` load projects this way.
Each fallback shows up in the Critic as a warning ("Content this version
doesn't know"), and `render` prints them. Editing stays strict:
`rosaclef validate` and the studio's server still reject the unknown keys,
so an agent's typo is caught. While a song holds content that a fix would
lose, fixes are refused. Suppressing still works, and the command line then
rewrites only the `critic` section of the file.

## Tests

`crates/core/src/critic/tests.rs`:

- Every check fires on a project built to trigger it.
- Every fix makes its own finding go away and leaves a valid project, on
  these projects and on the demo song.
- Suppressions work.
- JSON Patch is applied correctly, and the endpoint answers.

`compat.rs` has its own tests, and `web/test/critic-smoke.mjs` drives the
panel in a browser.

## How it reads a project

- **Pitches as they sound.** A channel's notes are shifted by the song's
  transpose and, for sampled instruments, the instrument's own transpose. The
  checks then compare them to real registers (C4 = MIDI 60). Fixes move the
  written notes by the same steps.
- **Roles.** Each channel gets a role from its instrument and notes:
  - **drums**: the drum machine, or a General MIDI kit.
  - **bass**: mostly single notes, with a median under D3.
  - **harmony**: 1.8 or more notes per onset.
  - **lead**: single notes, with a median of G3 or above.
  - **part**: any other pitched channel.
  - Transitions, the generative instrument, arpeggiated channels and
    samplers are kept out of the pitch checks. A layer counts with the
    channel it layers.
- **Parts and the song.** The note checks run on each channel's notes in each
  pattern. The song-wide checks (clashes, crowding, arrangement) expand the
  playlist's clips into the notes as they play, leaving out muted tracks and
  channels. They use each pattern on its own when nothing is placed.
- **The key** is the score's key when one is set (unless the notes clearly
  disagree). Otherwise it is the best Krumhansl–Schmuckler fit of the
  duration-weighted pitch classes. Minor keys also allow the raised 6th and
  7th.
- **Parameters** read their catalog defaults when a device leaves them out.
- **Instrument ranges** come from a table in `gm.rs`, next to the program names.

## The checks

77 checks of the project (the 8 measured on a render are in
[mixcheck.md](mixcheck.md#findings-and-the-critic)). "Fix" is the one-click change. A dash means the check only reports.
*Off by default* marks the 7 checks a project has to turn on.

### Harmony

| Check | Flags | Fix |
|---|---|---|
| Low interval limits | Two notes of a chord a close interval apart, with the lower note under its limit: m2 E3, M2 E♭3, m3 C3, M3 B♭2, P4 B♭2, tritone B2, P5 B♭1, m6 G2, M6/m7/M7 F2, m9 E2, M9 E♭2 | Open the voicing: the upper notes go up an octave |
| Chords crowd the bass | A chord of three or more notes, two of them under C3, while a bass part exists | Raise the notes under C3 an octave |
| Jumpy voice leading | Consecutive chords whose top voice leaps a major 6th or more, or whose center moves a 5th or more. Only reported when inversions cut the total movement by a quarter | Revoice each chord with the inversion nearest the one before |
| Parallel fifths and octaves *(off by default)* | Two voices of a sampled (acoustic) part moving the same way into the same perfect 5th or octave. Skipped in power-chord parts and synth stacks, where fusion is the point | — |
| Gaps in the upper voices *(off by default)* | Adjacent upper voices of a chord more than an octave apart | Close the gaps by octaves |
| Notes outside the key *(off by default)* | At most 2 notes (or 5%) of a part outside the key, in a song that is otherwise in it: under 8% of notes outside, and a clear key fit | Snap them to the nearest note of the key |
| Key signature disagrees *(off by default)* | The score's key fits the notes much worse than the detected key does (by 0.15 of correlation) | Set the score's key |
| Sustained semitone clashes | Two parts holding notes a minor 2nd, 9th or 16th apart for at least a beat, 2 beats in all | — |

### Melody

These run on lead parts: their top line, one note per onset.

| Check | Flags | Fix |
|---|---|---|
| Melody range *(off by default)* | A span of more than 19 semitones | — |
| Leaps over an octave *(off by default)* | Consecutive notes more than an octave apart | — |
| Leaps that do not recover *(off by default)* | Two or more leaps of a minor 6th or wider that keep going, or leap again, instead of stepping back | — |
| A melody that never breathes | 8 bars or more without a rest (a gap of an eighth) | — |
| Monotone melody | 16 or more notes over 4 bars using at most two pitch classes | — |
| Beyond the instrument's range | Notes outside a real instrument's sounding range, for about 50 General MIDI programs: pianos, guitars, basses, strings, winds, brass, voices and mallets | Move them into range by octaves |

### Rhythm and MIDI hygiene

| Check | Flags | Fix |
|---|---|---|
| Robotic velocities | 8 or more notes of a rhythmic part (drums, or a median note of a beat or less) at one velocity | Add dynamics: downbeats strongest, off-beat 8ths at 82%, 16ths at 72%, plus a little variation |
| Everything at full velocity | 90% of a part at velocity 124 or more | Scale the velocities to 80% |
| Machine-gun runs | 8 or more fast hits (16ths) of one drum at one velocity | Alternate strong and weak hits |
| No ghost notes | A snare or hat part of 8 or more hits, none softer than velocity 76 | — |
| Live instrument on the grid | 16 or more notes of a sampled acoustic instrument, all exactly on 16ths | Humanize the timing, by up to ±8 ms |
| Notes just off the grid | A part of 8 or more notes, 80% on 16ths, with the rest within 3% of a beat of them | Snap them to the grid |
| Overlapping notes | A note still sounding when the same pitch starts again (not drums) | Trim it to where the next begins |
| Duplicate notes | Identical notes stacked at one start | Remove the duplicates |
| Silent notes | Velocity 0 | Remove them |
| Very short notes | Pitched notes shorter than a 64th | Lengthen them to a 16th |
| Notes after the pattern ends | Notes that start at or after the pattern's length | Remove them |
| Swing with nothing to swing | Swing on, with no note on an off-beat 16th | — |

### Arrangement

| Check | Flags | Fix |
|---|---|---|
| Nothing on the playlist | Patterns with notes, and no clips | Lay the patterns out one after another |
| The song is one loop | 16 bars or fewer | — |
| Loopitis | The same 1-, 2-, 4- or 8-bar loop repeating note for note for 24 bars or more (a warning at 32) | — |
| No contrast between sections | Over 24 bars or more, the parts playing in each 8-bar block vary by fewer than 2 | — |
| Too many elements at once | More than 6 parts in a bar, with the drums counted as one and layers counted with their source | — |
| Everything enters at once | The first bar with sound already has 80% of the song's most parts (songs of 16 bars or more) | — |
| Abrupt ending | The last bar still has 80% of the most parts, with no automation in the last 4 bars | — |
| Clips just off the bar | A clip that starts up to half a beat away from a bar line | Snap it to the bar |
| Overlapping clips | Two pattern clips on one track that overlap | Trim the first one |
| Unused patterns | A pattern with notes that is never placed | — |
| Clips of empty patterns | Clips of a pattern with no notes | Remove them |
| Identical patterns | Two patterns with the same notes and length | Use the first in place of the copy, and delete the copy unless the score marks it |
| No automation | A song of 32 bars or more with no automation lanes | — |

### Low end

| Check | Flags | Fix |
|---|---|---|
| Bass below E1 | Pitched notes under E1 (41 Hz), a warning under C1 | Move them up an octave |
| Two parts in the sub | Two parts (not layers of one) under G2 (98 Hz) at the same time, for 4 beats in all | — |
| Kick and bass collide | Half the kicks or more land on a bass note that is already ringing, with no volume automation on the bass | — |
| Low end off center | A bass or kick panned more than 10% | Center it |
| Stereo width on the bass | A wavetable bass with unison spread over 30%. Also reported: analog unison on a bass, or a chorus or phaser on its insert | Narrow the unison to 15% |
| Reverb on the low end | A reverb over 10% wet on an insert that carries only kick and bass | Bring the reverb down to 5% |
| No low cut | An insert whose parts all play G3 or higher, with no high-pass filter and no low-shelf cut | Add a high-pass an octave under the lowest note (60–250 Hz) |
| Inharmonic FM bass | An FM bass with a non-integer operator ratio (not a half either) | — |

### Mix

| Check | Flags | Fix |
|---|---|---|
| Faders above unity | A channel or insert fader above 0 dB | Pull the faders down together, so the loudest is at unity and the balance stays |
| Every channel at the same level | Four or more channels all at one volume | — |
| Channels straight to the master | Three or more channels routed to the master | Give each its own insert (the drums share one) |
| Inserts with effects but no input | Effects on an insert that no channel or clip feeds | — |
| Solo left on | Any soloed insert | Turn the solos off |
| Muted parts | Muted channels with notes, muted tracks with clips, muted inserts | — |
| Automation that never moves | A lane whose points all have one value | Remove the lane |

### Stereo

| Check | Flags | Fix |
|---|---|---|
| Everything in the center | Three or more supporting parts and every channel and insert at center. The supporting parts are everything except kick, snare, clap, bass and the main lead | Spread them, alternating left and right by 20–55% |
| Lopsided stereo image | The volume-weighted pan of the parts (not kick and bass) more than 25% to one side | — |

### Effects

| Check | Flags | Fix |
|---|---|---|
| Reverb before dynamics | A reverb or delay before a compressor, EQ or drive on one insert | Move the delays and reverbs after the dynamics (the limiter stays last) |
| Delay off the beat | A delay time more than 3% from a note value (straight, dotted or triplet, a 64th to a whole note) | Snap it to the nearest note value |
| Runaway delay | Feedback of 85% or more | Lower it to 60% |
| Washed-out reverb | A reverb on the master over 15% wet (warning), or on an insert at 50% or more | Bring it down to 10% or 30% |
| Two reverbs on one insert | Two or more enabled reverbs | — |
| Crushing compression | A ratio of 8:1 or more with a threshold at −30 dB or lower | — |
| Compressor eats the drum transients | An attack under 3 ms on an insert that carries only drums | Set the attack to 10 ms |
| Big EQ boosts | A band boosted 9 dB or more | Halve the big boosts |
| Screaming resonance | Analog or wavetable filter resonance at 90% or more | — |
| Disabled effects | Bypassed devices | — |

### Master

| Check | Flags | Fix |
|---|---|---|
| Master fader above 0 dB | Master volume above unity | Set it to 0 dB |
| No limiter on the master | No enabled limiter | Add a limiter with a −1 dB ceiling |
| Limiter not last | An enabled device after the master limiter | Move the limiter to the end |
| No true-peak headroom | A limiter ceiling above −1 dB | Set the ceiling to −1 dB |
| Over-limited master | Limiter input gain above 6 dB | Drive it 3 dB |
| Heavy master chain | More than 4 enabled devices, or several limiters | — |

### Project

| Check | Flags | Fix |
|---|---|---|
| Tempo | A fractional tempo. Also reported: under 60 or over 200 BPM (half or double time?) | Round it |
| Patterns of odd lengths | A pattern that is not whole bars. Also reported: phrases of 3, 5 or 7 bars | Make it whole bars |
| Unused channels | Channels with no notes that nothing layers (samplers and plugins excepted) | — |
| Default names | "Pattern 3", "Channel 2", or an untitled song | — |
| Content this version doesn't know | Sections, fields, instruments, effects, parameters or option values that this version left out or replaced to play the song (see above) | — |

## Sources

The checks put into practice the
*Comprehensive Principles of Modern Music Production* report: voice leading,
parallel fifths, rootless voicings, the rule of threes, subtractive
arrangement, velocity and micro-timing, FM sub-bass, gain staging, M/S low
end, and LUFS and true peak. They also draw on these sources:

- Voice leading and counterpoint: [music21's `voiceLeading`](https://music21.org/music21docs/moduleReference/moduleVoiceLeading.html) and the [AP Music Theory rubric](https://apcentral.collegeboard.org/media/pdf/ap21-apc-music-theory-q6.pdf).
- Low interval limits: [Robin Hoffmann](https://www.robin-hoffmann.com/dfsb/low-interval-limits) (Berklee's table).
- Key finding: [Temperley on Krumhansl–Schmuckler](https://music.informatics.indiana.edu/courses/I546/pdf/temperley.pdf).
- Instrument ranges: [HyperPhysics](https://hyperphysics.gsu.edu/hbase/Music/orchins.html), and [MuseScore's out-of-range colors](https://musescore.org/en/node/32176).
- Humanizing MIDI: [DeMIDify](https://demidify.com/complete-guide-humanize-midi-drums.html) and [Mix Elite](https://mixelite.com/blog/humanizing-midi-drums/).
- Overlap fixes: Cubase's *Delete Overlaps (mono)* and Logic's *Remove Overlaps*.
- Arrangement: [Beat Kitchen](https://beatkitchen.io/guides/electronic-music/06-arrangement-and-energy/), [the Rule of Threes](https://music-chips.com/chips/the-rule-of-threes.html) and [Bobby Owsinski's arrangement rules](https://bobbyowsinskiblog.com/arrangement-rules/).
- Low end and stereo: [drolez's four-bucket rule](https://drolez.com/blog/music/four-bucket-rule-clear-mix.php), [Sonarworks on kick and bass](https://www.sonarworks.com/soundid-reference/blog/learn/eliminate-competition-between-the-kick-and-bass/) and [Mastering The Mix on stereo width](https://www.masteringthemix.com/pages/how-to-get-the-right-stereo-width).
- Chains and the master: [Mastering The Mix on the mastering chain](https://www.masteringthemix.com/blogs/learn/where-every-mastering-plugin-sits-in-the-chain) and [iZotope on mastering for streaming](https://www.izotope.com/en/learn/mastering-for-streaming-platforms.html).

Every threshold is a rule of thumb, not a law. Ignore a finding, or turn a
check off, when the music wants it the other way.
