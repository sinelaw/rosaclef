# Soundfonts

`gm/` is the General MIDI soundfont played by `soundfont` instruments:
MuseScore General 0.2 (MIT, see `gm/LICENSE.md`; the instruments and their
sample sources are in `gm/README.md` and `gm/SOURCES.csv`, as published
upstream), split for loading on demand by `tools/split_soundfont.py`. Every
preset is kept — the 128 General MIDI programs, 31 drum kits and the
variations on other banks (orchestral sections, extra pianos, guitars,
marching percussion) — except the "Expr." presets, whose loudness follows
MIDI controller 2. The studio names them in `crates/core/src/gm.rs`.

- `gm/index.sf2`: the whole soundfont without its sample data (presets,
  instruments, sample headers);
- `gm/smpl-NNN.bin`: the sample data (Ogg Vorbis), in 1 MiB pieces. Samples
  are ordered by the presets that use them, so a preset's samples are in a
  few neighbouring pieces.

To rebuild it:

```sh
base=https://ftp.osuosl.org/pub/musescore/soundfont/MuseScore_General
curl -LO $base/MuseScore_General.sf3
tools/split_soundfont.py MuseScore_General.sf3 web/soundfonts/gm
curl -L $base/MuseScore_General_Readme.md | tr -d '\r' > web/soundfonts/gm/README.md
curl -L $base/MuseScore_General_Sample_Sources.csv | tr -d '\r' > web/soundfonts/gm/SOURCES.csv
```
