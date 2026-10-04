#!/usr/bin/env python3
"""Split a SoundFont (SF2 or SF3) into the layout the studio loads lazily.

    tools/split_soundfont.py MuseScore_General.sf3 web/soundfonts/gm

writes

    OUT/index.sf2      the whole SoundFont without its sample data: INFO, an
                       empty `smpl` chunk and every preset, instrument and
                       sample header (a few hundred kB)
    OUT/smpl-NNN.bin   the sample data, cut into pieces of PIECE bytes

Sample headers keep pointing at offsets in the *whole* sample data, so a
loader reads the index, looks up the samples a preset uses and fetches only
the pieces covering them (`offset // PIECE`). The samples are reordered so
each preset's samples sit together: a song with a piano and a bass downloads
the piano's pieces and the bass's, not the whole bank.

Every bank is kept (General MIDI on bank 0, drum kits on bank 128, and the
variations on the other banks) except MuseScore General's "Expr." presets,
whose loudness follows MIDI controller 2, which Rosaclef does not send. The
samples of banks 0 and 128 come first, so adding the other banks leaves their
pieces as they were. The format is read by crates/engine/src/soundfont.rs
(see `PIECE` there).
"""

import struct
import sys
from pathlib import Path

PIECE = 1 << 20
# MuseScore General's expressive banks: loudness from CC2, which Rosaclef does not send.
EXPRESSIVE = (17, 18, 21, 26, 31, 41, 51)

# Record layouts of the pdta sub-chunks.
LAYOUT = {
    "phdr": "<20sHHHIII",
    "pbag": "<HH",
    "pmod": "<HHhHH",
    "pgen": "<HH",
    "inst": "<20sH",
    "ibag": "<HH",
    "imod": "<HHhHH",
    "igen": "<HH",
    "shdr": "<20sIIIIIBbHH",
}
ORDER = ["phdr", "pbag", "pmod", "pgen", "inst", "ibag", "imod", "igen", "shdr"]
GEN_INSTRUMENT = 41
GEN_SAMPLE = 53
OGG = 0x10


def chunks(buf, off, end):
    while off + 8 <= end:
        cid = buf[off : off + 4]
        ln = struct.unpack("<I", buf[off + 4 : off + 8])[0]
        yield cid, off + 8, ln
        off += 8 + ln + (ln & 1)


def read(path):
    data = Path(path).read_bytes()
    if data[:4] != b"RIFF" or data[8:12] != b"sfbk":
        sys.exit(f"{path}: not a SoundFont")
    info, smpl, pdta = [], b"", {}
    for cid, o, ln in chunks(data, 12, len(data)):
        if cid != b"LIST":
            continue
        kind = data[o : o + 4]
        for c2, o2, l2 in chunks(data, o + 4, o + ln):
            body = data[o2 : o2 + l2]
            if kind == b"INFO":
                info.append((c2, body))
            elif kind == b"sdta" and c2 == b"smpl":
                smpl = body
            elif kind == b"pdta":
                name = c2.decode()
                size = struct.calcsize(LAYOUT[name])
                pdta[name] = [list(struct.unpack(LAYOUT[name], body[i : i + size])) for i in range(0, len(body), size)]
    return info, smpl, pdta


def preset_samples(pdta, pi):
    """Sample indices a preset uses, in zone order."""
    phdr, pbag, pgen = pdta["phdr"], pdta["pbag"], pdta["pgen"]
    inst, ibag, igen = pdta["inst"], pdta["ibag"], pdta["igen"]
    out = []
    for b in range(phdr[pi][3], phdr[pi + 1][3]):
        for g in range(pbag[b][0], pbag[b + 1][0]):
            if pgen[g][0] != GEN_INSTRUMENT:
                continue
            ii = pgen[g][1]
            for b2 in range(inst[ii][1], inst[ii + 1][1]):
                for g2 in range(ibag[b2][0], ibag[b2 + 1][0]):
                    if igen[g2][0] == GEN_SAMPLE and igen[g2][1] not in out:
                        out.append(igen[g2][1])
    return out


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    src, out = sys.argv[1], Path(sys.argv[2])
    info, smpl, pdta = read(src)
    shdr = pdta["shdr"]
    presets = [i for i, p in enumerate(pdta["phdr"][:-1]) if p[2] not in EXPRESSIVE]
    # General MIDI and the drum kits first, then the variations.
    presets.sort(key=lambda i: (pdta["phdr"][i][2] not in (0, 128), pdta["phdr"][i][2], pdta["phdr"][i][1]))

    # Sample order: by first use, following the presets; stereo partners
    # right after each other.
    order = []
    for pi in presets:
        for s in preset_samples(pdta, pi):
            for t in (s, shdr[s][8]):
                if t < len(shdr) - 1 and t not in order:
                    order.append(t)

    # New sample data. PCM samples (SF2) are 16-bit frames with 46 zero
    # frames after each; Ogg samples (SF3) are byte ranges.
    data = bytearray()
    for s in order:
        h = shdr[s]
        if h[9] & OGG:
            a, b = h[1], h[2]
            start = len(data)
            data += smpl[a:b]
            h[1], h[2] = start, len(data)
        else:
            a, b = h[1] * 2, h[2] * 2
            start = len(data) // 2
            data += smpl[a:b] + bytes(92)
            shift = start - h[1]
            h[1], h[2], h[3], h[4] = start, h[2] + shift, h[3] + shift, h[4] + shift

    # Samples no kept preset uses have no data any more.
    kept = set(order)
    for s, h in enumerate(shdr[:-1]):
        if s not in kept:
            h[1] = h[2] = h[3] = h[4] = 0

    # Headers: drop the presets of other banks (instruments stay, they are small).
    phdr = pdta["phdr"]
    pdta["phdr"] = [phdr[i] for i in presets] + [phdr[-1]]
    # phdr bag indices stay valid: they point into the unchanged pbag list,
    # and each kept preset still ends where its successor in the file began.
    ends = {i: phdr[i + 1][3] for i in presets}
    pbag, pgen, pmod = [], [], []
    for n, i in enumerate(presets):
        pdta["phdr"][n] = list(phdr[i])
        pdta["phdr"][n][3] = len(pbag)
        for b in range(phdr[i][3], ends[i]):
            g0, g1 = pdta["pbag"][b][0], pdta["pbag"][b + 1][0]
            m0, m1 = pdta["pbag"][b][1], pdta["pbag"][b + 1][1]
            pbag.append([len(pgen), len(pmod)])
            pgen += pdta["pgen"][g0:g1]
            pmod += pdta["pmod"][m0:m1]
    pdta["phdr"][-1] = list(phdr[-1])
    pdta["phdr"][-1][3] = len(pbag)
    pbag.append([len(pgen), len(pmod)])
    pgen.append([0, 0])
    pmod.append([0, 0, 0, 0, 0])
    pdta["pbag"], pdta["pgen"], pdta["pmod"] = pbag, pgen, pmod

    def chunk(cid, body):
        return cid + struct.pack("<I", len(body)) + body + (b"\0" if len(body) & 1 else b"")

    def riff_list(kind, body):
        return chunk(b"LIST", kind + body)

    info_body = b"".join(chunk(c, b) for c, b in info)
    sdta_body = chunk(b"smpl", b"")
    pdta_body = b"".join(chunk(n.encode(), b"".join(struct.pack(LAYOUT[n], *r) for r in pdta[n])) for n in ORDER)
    riff = b"sfbk" + riff_list(b"INFO", info_body) + riff_list(b"sdta", sdta_body) + riff_list(b"pdta", pdta_body)
    out.mkdir(parents=True, exist_ok=True)
    for old in out.glob("smpl-*.bin"):
        old.unlink()
    (out / "index.sf2").write_bytes(b"RIFF" + struct.pack("<I", len(riff)) + riff)
    for k in range(0, len(data), PIECE):
        (out / f"smpl-{k // PIECE:03d}.bin").write_bytes(data[k : k + PIECE])
    print(
        f"{len(presets)} presets, {len(order)} samples, {len(data) / 1e6:.1f} MB in {(len(data) + PIECE - 1) // PIECE} pieces → {out}"
    )


if __name__ == "__main__":
    main()
