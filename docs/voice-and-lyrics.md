# Voice: lyrics, singing and speech (research and design)

Status: implemented. §8 says what was built and where it differs from this
plan.

The goal is a **Voice** feature that sings or speaks words in time with the
track. The lyrics, how they are split into syllables, which notes they fall on,
their pronunciation (phonemes) and, where wanted, exact phoneme timing all live
in `project.json` once. Everything a singing or speech engine needs is derived
from that one copy:

- a built-in voice that plays live in the browser and natively;
- local neural engines, such as DiffSinger voicebanks or Piper and Kokoro speech;
- external editors, such as Synthesizer V, VOCALOID, OpenUtau, ACE Studio and
  VoiSona;
- AI song generators, such as DiffRhythm, ACE-Step, YuE and Suno;
- karaoke and lyric formats, such as MIDI lyrics, MusicXML, UltraStar, LRC and
  TTML.

Two changes to the project model come with this, and both are useful beyond
voice:

1. **Reuse by reference.** A pattern can use another pattern, or a range of it,
   shifted in time and pitch, instead of copying its notes.
2. **Verses.** A pattern's lyric line can hold several verses. Each place the
   pattern plays chooses one.

---

## 1. What exists today

### 1.1 Conventional formats: lyrics attached to notes

Every serious format attaches **one syllable to the note it starts on**. A
syllable held over several notes (a melisma) is marked on the notes after the
first. Rests are gaps, with UTAU as the only exception. Only singing-synth
formats carry phonemes, and each uses its own alphabet.

| Format | Time unit | Lyric unit | Melisma (held syllable) | Phonemes | Language |
|---|---|---|---|---|---|
| SMF 1.0 `FF 05` lyric (RP-017, XF) | ticks (PPQ) | syllable; a trailing space ends a word | no event on the held notes | — | per file (`{@LATIN}`, `$Lyrc` charset) |
| .KAR | ticks | syllable in a `FF 01` text event | none | — | `@L` per file |
| MIDI 2.0 UMP / Clip File | delta clock | UTF-8 syllable, sent at the note's start | a Complete UMP of `0x00` | — | BCP 47 per channel |
| MusicXML 4.0 `<lyric>` | divisions | syllable + `<syllabic>` begin/middle/end/single | `<extend type="start/continue/stop">` | — | `xml:lang` per syllable |
| MEI `<verse><syl>` | dur | syllable, `@wordpos` | `@con="u"` | — | `xml:lang` |
| VOCALOID VSQX / VPR | 480 PPQ | syllable/word | lyric `-` | X-SAMPA + lock flag | voicebank |
| UTAU .ust | 480 PPQ, notes in sequence | voicebank alias (`か`, `a ka`) | repeated vowel (non-standard) | the alias is the phoneme | — |
| OpenUtau .ustx | 480 PPQ | syllable/word | `+~` (`+` = next syllable of the word) | `lyric[ph ph]`, per-phoneme offsets | phonemizer per track or note |
| Synthesizer V .svp | blicks (705 600 000 per quarter) | word/syllable | `-` (`+` = next syllable of the word) | `phonemes` string, per-phoneme duration and strength | `languageOverride` per note |
| CeVIO / VoiSona | 960 PPQ | kana/syllable | `ー` | `Phonetic` / `Phoneme` | — |
| UltraStar .txt | beat = ¼ of `#BPM`'s beat, + `#GAP` ms | syllable, with its trailing space | `~` (convention) | — | per file |
| LRC / enhanced (A2) LRC | mm:ss.xx | line / word | — | — | — |
| Apple TTML (`itunes:timing="Word"`) | clock time | word/syllable spans | — | — | `xml:lang` |

Lessons:

- **Store meaning, not characters.** The same character means different things
  in different formats. `-` is a melisma in VOCALOID and Synthesizer V but a
  phrase break in UltraStar. `+` holds a syllable in OpenUtau but starts the
  next syllable in Synthesizer V. `R` is a rest in UST but a rap note in
  UltraStar. The project should store facts ("this note holds the previous
  syllable", "this syllable ends a word"), and each exporter should write its
  format's markers.
- **Syllable to note is one to many.** Several syllables on one note
  (MusicXML's `<elision>`) is rare, and joining their text covers it.
- **Phonemes need their alphabet with them.** X-SAMPA, ARPAbet, romaji, pinyin
  phones and IPA do not map one to one. For example, Synthesizer V's `br` and
  `cl` have no VOCALOID equivalent. Store IPA, or say which alphabet is used,
  and convert through mapping tables. utaformatix3 has such tables.
- **Vocal lines are monophonic.** No singing format allows overlapping notes in
  one voice.
- **Hyphens are not syllable marks** in MusicXML or MIDI 2.0. Strip them when
  importing old SMF or KAR files.
- **Encoding varies.** UST and VSQ are Shift-JIS, SMF is usually undeclared,
  and UltraStar must be UTF-8.

### 1.2 Current AI models: four input styles

| Style | Who | What they need |
|---|---|---|
| **Notes + phonemes + durations** | DiffSinger (Apache-2.0, ONNX), NNSVS/ENUNU (MIT; HTS labels from MusicXML), TCSinger 2, SoulX-Singer (Apache-2.0, Feb 2026) | phoneme sequence, notes, optional phoneme durations (a duration model predicts missing ones), optional f0 and curves (energy, breathiness, voicing, tension, gender) |
| **Notes + lyric per note** | Synthesizer V 2, VOCALOID 6, ACE Studio 2, VoiSona, OpenUtau | a project file or MIDI/MusicXML with lyrics. The engine phonemizes; optional phoneme overrides. |
| **Timed or tagged lyric text, no notes** | DiffRhythm (**LRC** line times), JAM (word `[{start,end,word}]`, non-commercial), ACE-Step 1.5 (MIT), YuE, LeVo, SongBloom, Suno (`[Verse]`/`[Chorus]` tagged text + style prompt) | line or word start times, or just section-tagged text. Some accept a melody (YuE2: ABC notation). |
| **Text / SSML → audio + timestamps** | Piper (GPL-3.0+, ONNX, espeak-ng IPA), Kokoro-82M (Apache-2.0, runs in the browser via kokoro-js, about 88 MB q8), Kitten TTS (about 25 MB), cloud TTS (Azure word boundaries, ElevenLabs character alignment) | text, or IPA phonemes via SSML `<phoneme>`. They return word or character times. |

DiffSinger is the clearest picture of what a neural singer consumes. A `.ds`
segment holds:

- `ph_seq`: the phonemes, with `SP` for silence and `AP` for breath;
- `ph_dur`: seconds per phoneme;
- `ph_num`: phonemes per group;
- `note_seq`, `note_dur`, `note_slur`: the notes, with `note_slur` marking a
  melisma;
- optional curves.

A DiffSinger group **starts at the vowel on the note's start**. The consonants
before it belong to the previous group, so consonants sound *before* the beat.
NNSVS ("time-lag") and OpenUtau (negative phoneme positions) do the same.
Phoneme timing is therefore naturally **seconds relative to the note's start,
often negative**, and it does not scale with tempo.

Small and runnable locally:

- Speech: Piper, Kokoro and Kitten run in onnxruntime-web, so they work in the
  browser build.
- Singing: no open singing model is packaged for the browser. DiffSinger is
  ONNX and OpenUtau runs it on CPU, so it is the realistic native backend. A
  voicebank is roughly 100–300 MB (an estimate).
- Song generators and zero-shot singers are GPU workloads, so Rosaclef talks to
  them through exported files.

G2P (spelling to phonemes) that fits a GPL-3.0-or-later project:

- espeak-ng (GPL-3.0+, with an emscripten build);
- CMUdict (BSD-style);
- g2p_en and misaki (Apache-2.0);
- pyopenjtalk for Japanese and pypinyin for Mandarin (MIT);
- DiffSinger and OpenUtau dictionaries (Apache-2.0 / MIT).

Model weights and voicebanks have their own terms, often non-commercial. Users
supply them; Rosaclef never bundles them.

Alignment, to get lyric timing from a recording:

- SOFA (MIT, built for singing; TextGrid/HTK output, feeds DiffSinger);
- Montreal Forced Aligner (MIT, speech, TextGrid);
- WhisperX (word JSON; needs separated vocals);
- SingAlign (checkpoints are non-commercial).

---

## 2. Reuse by reference

Today a clip references a pattern by id, and a pattern loops to fill the clip.
Anything finer is copied: a motif used in two patterns, the same phrase a
fourth higher, the bass doubling the melody an octave down. The proposal is a
`uses` list on a pattern:

```jsonc
{ "id": "hook", "name": "Hook", "length": 4, "notes": [ /* written once */ ] },
{ "id": "verse", "name": "Verse", "length": 16,
  "notes": [ { "channel": "lead", "pitch": 64, "start": 0, "length": 1 } ],
  "uses": [
    { "pattern": "hook", "start": 4, "transpose": 5 },
    { "pattern": "hook", "from": 2, "to": 4, "start": 12,
      "channel": "bass", "transpose": -12, "velocity": 0.7 }
  ] }
```

| field | meaning |
|---|---|
| `pattern` | the pattern to play (required) |
| `start` | where it starts in this pattern, in beats |
| `from`, `to` | the range of the used pattern, in its own beats (default: all of it). Notes are cut at the edges, as a clip with an offset does. |
| `transpose` | semitones (drum channels are not shifted, as with `transport.transpose`) |
| `channel` | play every note on this channel instead of its own |
| `velocity` | a factor on the used notes' velocities |
| `verse` | which verse its lyrics sing (see §3) |

Rules:

- **References are resolved in `rosaclef-core`, the same way `form.rs` unrolls
  repeats.** A new `expand.rs` turns a pattern into plain notes (each note
  remembers where it came from, as the score's notes already remember their
  pattern note).
  - The engine's `set_project` compiles from the expanded notes, so the audio
    path does not change.
  - The score, the exporters, `summary` and the Voice device read the same
    expanded notes, so everything agrees on what plays.
- **`uses` is a separate list, not entries mixed into `notes`.** `notes[i]`
  stays a plain note, so `NoteIx`, the piano roll's selection and
  `.rosaclef/context.json` keep their meaning. Hand-writing a reference is just
  as easy either way.
- **Validation.** `rosaclef validate` reports, with JSON paths:
  - an unknown pattern id;
  - a cycle (a pattern that uses itself, directly or through others), so
    expansion always terminates;
  - a range outside the used pattern;
  - a channel that does not exist.
- **Editing.**
  - The piano roll draws used notes as ghost notes, grouped in a labeled box,
    and clicking the box opens the source pattern.
  - **Make unique** turns a reference into plain notes.
  - **Make reference** replaces a selection with a new pattern plus a use. That
    is how a copy-paste becomes a link.
- **Agents.** `rosaclef summary --expand PATTERN` prints the notes a reference
  produces, and `AGENTS.md` documents `uses`.

Ranges of the *song* (bars 9–16 again at bar 33) are already clips with an
`offset`, so they need nothing new. Reusing automation lanes the same way is a
possible follow-up, not part of this work.

---

## 3. Lyrics

### 3.1 Where they live

Lyrics belong to a **pattern**, per vocal channel, so they travel with the
notes when the pattern is used anywhere:

```jsonc
{ "id": "verse", "name": "Verse", "length": 16, "notes": [ ... ],
  "lyrics": [
    { "channel": "lead",
      "lang": "en-US",
      "verses": {
        "1": "Hel-lo dark-ness my old friend _ / I've come to talk with you a-gain",
        "2": "Be-cause a vi-sion soft-ly creep-ing _ / left its seeds while I was sleep-ing"
      },
      "syllables": [
        { "verse": 1, "at": 5.5, "phonemes": "f r ɛ n d", "kind": "sing" }
      ] }
  ] }
```

- **`verses`** maps verse numbers to lyric text in the notation below.
- The syllables are matched to the channel's notes **in time order**, after
  `uses` are expanded. Each note gets the next syllable, the way a hymnal or
  MusicXML lays verses under a melody.
  - Moving a note keeps its syllable.
  - Inserting or deleting notes re-flows the words.
- **`syllables`** holds optional overrides for one syllable, anchored by
  `verse` and `at`. `at` is the beat of the note in the pattern; a vocal line
  is monophonic, so a beat identifies a note.
- **`lang`** is a BCP 47 language tag. It drives G2P and exports such as
  MusicXML `xml:lang`, MIDI 2.0 Lyrics Language and Synthesizer V
  `languageOverride`. An override can change it for one syllable.

### 3.2 The text notation

The notation is plain text that a person or an agent can type. It follows the
hymnal and MusicXML conventions:

| text | meaning | stored fact |
|---|---|---|
| space | ends a word | `syllabic` end/single |
| `-` inside a word (`Hel-lo`) | a syllable break within the word | `syllabic` begin/middle |
| `_` | this note holds the previous syllable (melisma) | `melisma` |
| `/` | end of a lyric line | line end (CR in SMF and MIDI 2.0, `- beat` in UltraStar, a new LRC line) |
| `//` | end of a paragraph or section | paragraph end |
| `word[ph ph]` | a pronunciation override in IPA, or in the line's `alphabet` (the same syntax as OpenUtau's phoneme hints) | `phonemes`, locked |
| `(br)` | a breath on its own note | `kind: breath` (Synthesizer V `br`, DiffSinger `AP`) |
| `\-`, `\_`, `\/` | the literal character | — |

Validation warns when a verse has more syllables than the line has notes, or
fewer, so silent notes are never an accident. It reports overlapping notes on a
channel that has lyrics.

### 3.3 Verses and where each one plays

Which verse a pattern sings is decided by the first of these that is set:

1. the **use** that brings it in (`"verse": 2`);
2. the playlist **clip** (`"verse": 2`);
3. the **pass of a repeat**: pass 2 of `|: … :|` sings verse 2, the way
   MusicXML's `time-only` and stacked verses under a repeated melody work;
4. verse 1.

This lets one 16-beat pattern carry all the verses of a strophic song, whether
it is placed on the playlist twice or played once inside a repeat.

### 3.4 Pronunciation and timing are layered

| layer | stored? | where it comes from |
|---|---|---|
| words and syllables on notes | always (the text) | the user or agent |
| phonemes | only overrides | otherwise G2P at render time: a dictionary (CMUdict, DiffSinger/OpenUtau dictionaries, user entries), with espeak-ng as fallback |
| phoneme timing | only when locked | otherwise the engine decides (its duration model or phonemizer). Locked timing comes from forced alignment or a manual edit. |

Phoneme timing is stored as **seconds relative to the note's start**, which can
be negative for consonants before the beat. A `role` (onset, nucleus or coda)
lets exporters find DiffSinger's vowel-start groups:

```jsonc
{ "verse": 1, "at": 8, "phonemes": [
    { "p": "f", "offset": -0.09, "role": "onset" },
    { "p": "ɹ", "offset": -0.04, "role": "onset" },
    { "p": "ɛ", "role": "nucleus" },
    { "p": "n", "role": "coda" }, { "p": "d", "role": "coda" } ] }
```

Predicted phoneme durations are derived data. They are cached under
`.rosaclef/`, not written into `project.json`, so the file holds only what a
person decided.

### 3.5 Speech and rap

Notes give the timing. Their pitch is ignored for `"mode": "rap"` (UltraStar
`R`, Synthesizer V rap) and `"mode": "speak"`, both set on the lyric line.

A speak line on a single note says the whole line starting at that note (TTS),
which suits narration, spoken intros and announcements. The engine reports the
word times it produced, and the UI shows them under the note.

### 3.6 Expression

Expression uses the automation that already exists. The Voice device has
parameters (`breathiness`, `tension`, `voicing`, `gender`, `vibrato`, `energy`),
so `channel/<id>/breathiness` lanes drive them live. The same lanes are
resampled into DiffSinger's `*_timestep` curves and Synthesizer V's parameter
curves on export. Pitch comes from the notes, plus a future pitch-bend lane.

---

## 4. The Voice device

The device is a new instrument type in the catalog, `"type": "voice"`. Its
working name is **Voix**, matching the other device names. It is a channel like
any other, so it routes to an insert, gets effects, mutes and solos.

| `options.engine` | what | where | latency |
|---|---|---|---|
| `formant` (default) | a built-in formant voice (Klatt-style source–filter with diphone transitions), driven by phonemes from the built-in G2P. Intelligible but synthetic: a guide vocal. | engine crate: native **and** WebAssembly | real time |
| `diffsinger` | a user-supplied DiffSinger voicebank (`dsconfig.yaml`, ONNX), run with ONNX Runtime | native first; the browser later via onnxruntime-web | renders phrases ahead |
| `speech` | Piper / Kokoro / Kitten models | native, and the browser via onnxruntime-web | renders phrases ahead |
| `external` | export the line (see §5), sing it in Synthesizer V, OpenUtau, ACE Studio or a song generator, and drop the WAV back. The device plays the returned stem in place. | anywhere | manual |

Neural engines **render by phrase**. A phrase is the notes between rests, like
DiffSinger's segments.

- Each phrase is rendered to `renders/.voice/<hash>.wav`, where the hash covers
  the phrase's notes, syllables, phonemes, tempo, curves and engine settings.
- The engine plays the cached phrase as it plays audio clips today, following
  the tempo map.
- An edit re-renders only the phrases it touched. Until the new render is
  ready, that phrase plays with the formant voice, so playback never stops
  waiting.
- Offline renders (`rosaclef render`) wait for every phrase.

The real-time engine needs one addition. Compiled notes (`CNote`) get an index
into a per-channel syllable table (phonemes, kind, locked offsets), so a
`voice` instrument receives "note on, with these phonemes" and can start the
onset consonants early. Phoneme lookups happen in `set_project`, not on the
audio thread.

CLAP and VST3 have no lyric event. Vendor bridges (ACE Studio's, Synthesizer V's
plugin) sync the transport only. Lyrics therefore reach external singers
through files, not through the plugin host.

---

## 5. One source, many exports

Every export below is computed from the expanded notes, the chosen verses and
the tempo map. None of them needs data the project does not have.

| target | export | notes |
|---|---|---|
| MIDI (SMF 1) | `FF 05` per syllable, written before the note-on at the same tick. A trailing space ends a word; CR/LF end lines and paragraphs. One track per vocal channel. | **Import too.** `rosaclef import-midi` currently skips lyric events (`crates/import/src/midi.rs` reads only `0x03`/`0x04`). Handle `FF 05` and KAR `FF 01`, strip hyphens, and detect the encoding. |
| MusicXML | `<lyric number="v">` with `<syllabic>`, `<text xml:lang>`, `<extend>`; verses stacked | also feeds Sinsy/NNSVS, VoiSona, Synthesizer V and ACE Studio |
| Score view and PDF | lyrics under the staff, verses stacked, hyphens and extender lines drawn | `web/src/engrave.js` |
| OpenUtau `.ustx` | notes with `lyric`, `+` / `+~` extenders, `word[ph]` when phonemes are locked | OpenUtau drives DiffSinger, ENUNU and UTAU banks |
| Synthesizer V `.svp` | notes in blicks, `-` / `+`, `phonemes`, `languageOverride`, parameter curves from automation | the most complete target |
| DiffSinger `.ds` | segments split at rests: `ph_seq` (with a `lang/` prefix), `ph_num` (vowel-start groups), `note_seq/dur/slur`, `ph_dur` when locked or predicted, curves | `ph_dur` comes from locked timing or the voicebank's duration model |
| UltraStar `.txt` | `: beat len pitch syl`; one BPM (×4) + `#GAP`; `-` line breaks; `F`/`R` for speak and rap | tempo changes are flattened |
| LRC / enhanced LRC | line times, `<mm:ss.xx>` word times | DiffRhythm input; karaoke players |
| JAM word JSON | `[{start, end, word}]` | |
| Tagged lyrics | `[Verse]` / `[Chorus]` paragraphs, named from the pattern or a section label | ACE-Step, YuE, LeVo, SongBloom, Suno |
| ABC notation | the vocal melody | YuE2-style melody plans |
| SSML | `<s>` per line, `<phoneme alphabet="ipa">`, `<break>` from gaps, `<mark>` per word | the timestamps returned are written back as word times |
| Apple TTML / WebVTT | word spans | |

Import paths into the same model:

- MIDI and KAR lyrics.
- MusicXML lyrics.
- `.ustx`, `.svp` and UltraStar.
- **Aligned recordings**: TextGrid (SOFA, MFA), WhisperX JSON, or ElevenLabs
  and Azure timings. These fill in locked phoneme or word timing.
- **The Voice tab (F8).** Melody transcription already cuts a take into
  syllables (`transcribe.rs`). Pasting the lyric text binds one syllable to
  each transcribed note, turning a sung take into notes, lyrics and timing in
  one step.

---

## 6. Plan

Each step is useful on its own.

1. **Model.** Pattern `uses` (`expand.rs`, validation, schema, engine, score
   and piano roll read expanded notes, Make unique and Make reference) and
   `lyrics` with verses (notation parser in core, validation, verse choice by
   use, clip and repeat pass). `AGENTS.md` documents both, so an agent can
   write "verse 2 of the chorus" on day one.
2. **See and type lyrics.** The score draws them. In the piano roll, a lyric
   row under the notes lets you type a syllable, press Tab to go to the next
   note, and press `_` to hold. MIDI lyric import and export.
3. **Exports.** MusicXML, `.ustx`, `.svp`, UltraStar, LRC, tagged text, `.ds`,
   SSML. The project then works with whatever singer the user already has.
4. **The `formant` voice.** A real-time guide singer in the engine, with
   built-in English G2P (CMUdict subset + rules, with espeak-ng optional), in
   the browser and natively.
5. **Neural engines.** DiffSinger and speech models through ONNX Runtime with
   the phrase cache, natively first. `external` round-trip with a watched drop
   folder.
6. **Alignment import.** TextGrid and WhisperX; lyrics from the Voice tab.

## 7. Decisions on the open questions

- **Verse keys are numbers only.** A section's name comes from its pattern's
  name ("Verse", "Chorus"), which tagged-lyrics exports use.
- **Lyrics live only in patterns.** A word split across two patterns is
  written as two words; uses already let a line span reused material.
- **IPA is canonical.** Pronunciations in the text (`word[w ɜ d]`) and stored
  timing are IPA; exporters convert (ARPAbet for English singers, X-SAMPA,
  romaji).
- **The dictionary is bundled.** 20 000 frequent words plus their
  contractions (about 460 kB, `tools/gen_dict.py`) ship in the engine, so the
  voice sings in the browser build with no download; other words go through
  spelling rules.

## 8. What was built

| step | where |
|---|---|
| uses, lyrics, verses, validation, schema | `crates/core`: `expand.rs`, `lyrics/`, `validate/`, `form.rs` (each span knows its pass) |
| the mirror in the studio | `web/src/expand.js`, `web/src/lyrics.js` (tested against the same notation cases) |
| piano roll, score, playlist, Voice tab | `web/src/ui/` |
| pronunciation | `crates/phonetics`: English dictionary + rules, Spanish, Japanese kana/romaji; ARPAbet, X-SAMPA |
| the song as sung lines, exports, alignment | `crates/vocal`: `line.rs`, `formats/`, `align/` |
| MIDI and karaoke lyric import | `crates/import/src/karaoke.rs` |
| the formant voice | `crates/engine/src/instruments/voice/` |
| rendered phrases | `crates/core/src/phrase.rs`, `crates/vocal/src/phrase.rs`, the voice's `render.rs`, `crates/server/src/voices.rs` |
| commands and API | `rosaclef export`, `rosaclef align`, `rosaclef summary --expand`, `GET /api/export` |

Where it differs from the plan:

- **Engines are commands, not linked libraries.** Instead of ONNX Runtime in
  the server, a voice is any program the user sets up in
  `~/.config/rosaclef/voices.json`: it reads one of the export formats for a
  phrase and writes a WAV. DiffSinger's own inference script, Piper, a cloud
  TTS wrapper or a song generator all fit without adding a heavy native
  dependency, and no model ever ships with Rosaclef. Commands come only from
  the user's settings, never from a project, so opening a shared project
  runs nothing. The browser build plays renders made elsewhere but cannot run
  engines.
- **"External" is a voice without a command.** The studio writes each
  phrase's input file next to where its render goes; singing it in another
  program and saving `<key>.wav` there is the round trip.
- **Phrase audio starts 0.3 s before the first note.** The engine cues it
  ahead of the note so consonants land before the beat; when the cue is
  missed (playback started mid-phrase) it starts at the note, in place.

Known limits:

- Renders follow the project tempo, not tempo automation.
- Exports ignore swing, MusicXML quantizes to sixteenths (no triplets) and
  assumes one meter, and MIDI export writes the first tempo only.
- Formats with one voice (karaoke, timed text, speech, DiffSinger) write the
  first channel that sings.
- The OpenUtau and Synthesizer V files follow the formats as documented but
  have not been opened in those programs.
- Other languages than English, Spanish and Japanese are read with English
  spelling rules.

## Sources

Conventional formats:
[RP-017](https://midi.org/smf-lyric-meta-event-definition/),
[XF spec v2.01](https://musescore.org/sites/musescore.org/files/2023-09/xfspec.pdf),
[UMP / MIDI 2.0 v1.1.1](https://amei.or.jp/midistandardcommittee/MIDI2.0/MIDI2.0-DOCS/M2-104-UM_v1-1-1_UMP_and_MIDI_2-0_Protocol_Specification.pdf),
[MIDI 2.0 melisma thread](https://midi.org/community/midi-specifications/midi-2-0-lyric-data-message-melisma-questions),
[KAR formats](https://www.mixagesoftware.com/en/midikit/help/HTML/karaoke_formats.html),
[MusicXML `<lyric>`](https://www.w3.org/2021/06/musicxml40/musicxml-reference/elements/lyric/),
[MusicXML `<extend>`](https://www.w3.org/2021/06/musicxml40/musicxml-reference/elements/extend/),
[MEI `<syl>`](https://music-encoding.org/guidelines/v5/elements/syl.html),
[utaformatix3](https://github.com/sdercolin/utaformatix3),
[OpenUtau](https://github.com/stakira/OpenUtau),
[ustx schema](https://www.schemastore.org/openutau-ustx.json),
[Synthesizer V scripting: Note](https://resource.dreamtonics.com/scripting/Note.html),
[UltraStar format](https://usdx.eu/format/),
[LRC](https://en.wikipedia.org/wiki/LRC_(file_format)),
[TTML lyrics](https://composer.betterlyrics.org/guides/ttml-file-format-spec),
[WebVTT](https://www.w3.org/TR/webvtt1/).

Models and tools:
[DiffSinger](https://github.com/openvpi/DiffSinger),
[NNSVS](https://arxiv.org/abs/2210.15987),
[pysinsy](https://github.com/r9y9/pysinsy),
[ENUNU](https://github.com/oatsu-gh/ENUNU),
[SoulX-Singer](https://github.com/Soul-AILab/SoulX-Singer),
[TCSinger 2](https://arxiv.org/abs/2505.14910),
[YingMusic-Singer](https://arxiv.org/abs/2512.04779),
[Vevo2](https://arxiv.org/abs/2508.16332),
[VocalRender](https://arxiv.org/abs/2607.27768),
[DiffRhythm](https://github.com/ASLP-lab/DiffRhythm),
[JAM](https://github.com/declare-lab/jamify),
[ACE-Step 1.5](https://github.com/ace-step/ACE-Step-1.5),
[VOCALOID6 formats](https://www.vocaloid.com/en/support/faq/603),
[VoiSona import](https://manual.voisona.com/en/song/pc/2b6e9bc7efb180f79e77f218506dafc3),
[Synthesizer V 2.2.0](https://forum.dreamtonics.com/t/news-synthesizer-v-studio-2-pro-2-2-0-new-choir-voices-collections/7437),
[kokoro-js](https://npmjs.com/package/kokoro-js),
[ElevenLabs timestamps](https://elevenlabs.io/docs/api-reference/streaming-with-timestamps),
[SOFA](https://github.com/qiuqiao/SOFA),
[SingAlign](https://huggingface.co/pymaster/SingAlign).

Not verified: Synthesizer V's import list beyond MIDI, MusicXML and SVP (the
docs site was blocked); ACE Studio's file format and any API; DiffRhythm 2's
lyric format; the weight licenses of YingMusic, Vevo2, SongBloom and LeVo.
