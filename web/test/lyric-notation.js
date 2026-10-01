// Lyric notation cases shared by web/test/lyrics.test.js and
// crates/core/src/lyrics/notation.rs (which reads the array as JSON, so it
// stays JSON: double quotes, no trailing commas).
/** type LyricCase = { text: String, tokens: String[], error: Int } */

// prettier-ignore
/** const LYRIC_CASES: LyricCase[] */
export const LYRIC_CASES = [
  {
    "text": "Hel-lo dark-ness my",
    "tokens": [
      "Hel<",
      "lo>",
      "dark<",
      "ness>",
      "my"
    ],
    "error": -1
  },
  {
    "text": "beau-ti-ful",
    "tokens": [
      "beau<",
      "ti~",
      "ful>"
    ],
    "error": -1
  },
  {
    "text": "Hel- lo",
    "tokens": [
      "Hel<",
      "lo>"
    ],
    "error": -1
  },
  {
    "text": "Hel -lo",
    "tokens": [
      "Hel<",
      "lo>"
    ],
    "error": -1
  },
  {
    "text": "friend _ / I've (br) come // a- _ gain",
    "tokens": [
      "friend",
      "_/",
      "I've",
      "(br)",
      "come//",
      "a<",
      "_",
      "gain>"
    ],
    "error": -1
  },
  {
    "text": "love__",
    "tokens": [
      "love",
      "_",
      "_"
    ],
    "error": -1
  },
  {
    "text": "end/",
    "tokens": [
      "end/"
    ],
    "error": -1
  },
  {
    "text": "read[r ɛ d]",
    "tokens": [
      "read[r ɛ d]"
    ],
    "error": -1
  },
  {
    "text": "Hel[h ɛ]-lo[l oʊ]",
    "tokens": [
      "Hel[h ɛ]<",
      "lo[l oʊ]>"
    ],
    "error": -1
  },
  {
    "text": "well\\-known AC\\/DC x\\_y",
    "tokens": [
      "well-known",
      "AC/DC",
      "x_y"
    ],
    "error": -1
  },
  {
    "text": "  ",
    "tokens": [],
    "error": -1
  },
  {
    "text": "a [b]",
    "tokens": [],
    "error": 2
  },
  {
    "text": "ab[c",
    "tokens": [],
    "error": 2
  },
  {
    "text": "ab]",
    "tokens": [],
    "error": 2
  },
  {
    "text": "ab[ ]",
    "tokens": [],
    "error": 2
  }
];
