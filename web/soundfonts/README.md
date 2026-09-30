# Soundfonts

`gm/` is the General MIDI soundfont played by `soundfont` instruments:
MuseScore General 0.2 (MIT, see `gm/LICENSE.md`), split for loading on
demand by `tools/split_soundfont.py`:

- `gm/index.sf2`: the whole soundfont without its sample data (presets,
  instruments, sample headers);
- `gm/smpl-NNN.bin`: the sample data (Ogg Vorbis), in 1 MiB pieces. Samples
  are ordered by the presets that use them, so a preset's samples are in a
  few neighbouring pieces.

To rebuild it:

```sh
curl -LO https://ftp.osuosl.org/pub/musescore/soundfont/MuseScore_General/MuseScore_General.sf3
tools/split_soundfont.py MuseScore_General.sf3 web/soundfonts/gm
```
