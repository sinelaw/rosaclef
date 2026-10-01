#!/usr/bin/env python3
"""Generate crates/phonetics/data/en.dict: English pronunciations in IPA.

The entries come from the CMU Pronouncing Dictionary (BSD-style licence,
see crates/phonetics/data/CMUDICT-LICENSE); the words kept are the most
frequent ones of a subtitles word list (hermitdave/FrequencyWords, CC BY-SA
4.0), which suits sung and spoken lyrics, and their contractions. Each line is `word<TAB>phonemes`,
phonemes in IPA separated by spaces, sorted by word (bytewise) so the
library can binary-search the file as it is.

    tools/gen_dict.py                # downloads the sources
    tools/gen_dict.py --cmudict cmudict.dict --freq en_50k.txt --words 20000
"""

import argparse
import pathlib
import urllib.request

CMUDICT = "https://raw.githubusercontent.com/cmusphinx/cmudict/master/cmudict.dict"
FREQ = "https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018/en/en_50k.txt"
CONTRACTIONS = {"t", "m", "re", "ve", "ll", "d"}
OUT = pathlib.Path(__file__).resolve().parent.parent / "crates/phonetics/data/en.dict"

# ARPAbet → IPA (General American). Unstressed AH/ER are schwas.
IPA = {
    "AA": "ɑ", "AE": "æ", "AH": "ʌ", "AH0": "ə", "AO": "ɔ", "AW": "aʊ", "AY": "aɪ",
    "B": "b", "CH": "tʃ", "D": "d", "DH": "ð", "EH": "ɛ", "ER": "ɝ", "ER0": "ɚ",
    "EY": "eɪ", "F": "f", "G": "g", "HH": "h", "IH": "ɪ", "IY": "i", "JH": "dʒ",
    "K": "k", "L": "l", "M": "m", "N": "n", "NG": "ŋ", "OW": "oʊ", "OY": "ɔɪ",
    "P": "p", "R": "ɹ", "S": "s", "SH": "ʃ", "T": "t", "TH": "θ", "UH": "ʊ",
    "UW": "u", "V": "v", "W": "w", "Y": "j", "Z": "z", "ZH": "ʒ",
}  # fmt: skip


def read(source: str) -> list[str]:
    if source.startswith("https://"):
        with urllib.request.urlopen(source) as r:
            return r.read().decode("utf-8").splitlines()
    return pathlib.Path(source).read_text(encoding="utf-8").splitlines()


def ipa(arpabet: list[str]) -> str:
    out = []
    for ph in arpabet:
        base = ph.rstrip("012")
        out.append(IPA.get(base + "0", IPA[base]) if ph.endswith("0") else IPA[base])
    return " ".join(out)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--cmudict", default=CMUDICT)
    ap.add_argument("--freq", default=FREQ)
    ap.add_argument("--words", type=int, default=20000)
    ap.add_argument("--out", default=str(OUT))
    a = ap.parse_args()

    prons: dict[str, str] = {}
    for line in read(a.cmudict):
        parts = line.split("#")[0].split()
        if len(parts) < 2 or "(" in parts[0]:
            continue  # comments and alternative pronunciations
        prons.setdefault(parts[0].lower(), ipa(parts[1:]))

    kept: dict[str, str] = {}
    for line in read(a.freq):
        word = line.split(" ")[0].lower()
        if word in prons and word not in kept:
            kept[word] = prons[word]
        if len(kept) >= a.words:
            break

    # The word list splits contractions ("don't" → "don", "t"), which lyrics
    # are full of: keep those of the kept words ('s only for common ones,
    # not every possessive).
    rank = {w: i for i, w in enumerate(kept)}
    for word, pron in prons.items():
        base, _, suffix = word.partition("'")
        if base in rank and (suffix in CONTRACTIONS or (suffix == "s" and rank[base] < 2000)):
            kept.setdefault(word, pron)

    lines = sorted((f"{w}\t{p}" for w, p in kept.items()), key=lambda s: s.split("\t")[0].encode())
    pathlib.Path(a.out).write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"{len(lines)} words → {a.out}")


if __name__ == "__main__":
    main()
