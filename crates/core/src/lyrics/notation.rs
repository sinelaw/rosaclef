//! The lyric notation: how a verse is typed.
//!
//! | text | meaning |
//! |---|---|
//! | space | ends a word |
//! | `Hel-lo`, `Hel- lo` | a syllable break inside a word |
//! | `_` | a note that holds the previous syllable (melisma) |
//! | `/`, `//` | end of a line, of a paragraph |
//! | `word[w ɜ d]` | the syllable's pronunciation, in IPA |
//! | `(br)` | a breath, on its own note |
//! | `\-` `\_` `\/` `\[` `\(` `\\` | the character itself |
//!
//! Every token takes one note: a syllable, a hold or a breath.

use std::fmt;

/// One note's worth of lyrics.
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub sung: Sung,
    /// A line or paragraph ends after it.
    pub brk: Break,
}

impl Token {
    /// How it reads under a note: a syllable with a hyphen when its word goes
    /// on, `_` for a hold, `(br)` for a breath.
    pub fn reads(&self) -> String {
        match &self.sung {
            Sung::Syllable { text, pos, .. } if !pos.ends_word() => format!("{text}-"),
            Sung::Syllable { text, .. } => text.clone(),
            Sung::Hold => "_".into(),
            Sung::Breath => "(br)".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Sung {
    Syllable {
        text: String,
        pos: WordPos,
        /// IPA phonemes given in the text; empty: worked out from the spelling.
        phonemes: Vec<String>,
    },
    /// Holds the previous syllable over this note.
    Hold,
    Breath,
}

/// Where a syllable sits in its word (MusicXML's `<syllabic>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WordPos {
    Single,
    Begin,
    Middle,
    End,
}

impl WordPos {
    /// Whether a word ends with this syllable.
    pub fn ends_word(self) -> bool {
        matches!(self, WordPos::Single | WordPos::End)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Break {
    #[default]
    None,
    Line,
    Paragraph,
}

/// A mistake in a verse's text, at a character position.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    /// Characters from the start of the text.
    pub at: usize,
    pub message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at character {})", self.message, self.at + 1)
    }
}

/// Read a verse into tokens, one per note.
pub fn parse(text: &str) -> Result<Vec<Token>, ParseError> {
    let chars: Vec<char> = text.chars().collect();
    let mut p = Parser::default();
    let mut i = 0;
    while i < chars.len() {
        i = p.step(&chars, i)?;
    }
    p.end_syllable();
    p.end_word();
    Ok(p.tokens)
}

#[derive(Default)]
struct Parser {
    tokens: Vec<Token>,
    /// Token indices of the word being read, and of the last word read (a
    /// leading `-` joins it again).
    word: Vec<usize>,
    last_word: Vec<usize>,
    text: String,
    phonemes: Vec<String>,
    /// The last syllable ended with `-`: the word goes on.
    cont: bool,
}

impl Parser {
    /// Read the character at `i`; returns where to go on.
    fn step(&mut self, chars: &[char], i: usize) -> Result<usize, ParseError> {
        let c = chars[i];
        match c {
            c if c.is_whitespace() => {
                self.end_syllable();
                if !self.cont {
                    self.end_word();
                }
            }
            '-' => self.hyphen(),
            '_' => {
                self.end_syllable();
                self.push(Sung::Hold);
            }
            '/' => {
                let n = chars[i..].iter().take_while(|c| **c == '/').count();
                self.slash(if n > 1 { Break::Paragraph } else { Break::Line });
                return Ok(i + n);
            }
            '[' => return self.pronunciation(chars, i),
            ']' => return Err(error(i, "a ] without its [")),
            '(' if starts_with(chars, i, "(br)") => {
                self.end_syllable();
                self.end_word();
                self.push(Sung::Breath);
                return Ok(i + 4);
            }
            '\\' => {
                self.letter(chars.get(i + 1).copied().unwrap_or('\\'));
                return Ok(i + 2);
            }
            c => self.letter(c),
        }
        Ok(i + 1)
    }

    fn letter(&mut self, c: char) {
        self.text.push(c);
        self.cont = false;
    }

    fn hyphen(&mut self) {
        if self.text.is_empty() && self.word.is_empty() && !self.cont {
            // "Hel -lo": the hyphen joins the word before.
            self.word = std::mem::take(&mut self.last_word);
        }
        self.end_syllable();
        self.cont = true;
    }

    fn slash(&mut self, brk: Break) {
        self.end_syllable();
        self.end_word();
        if let Some(t) = self.tokens.last_mut() {
            t.brk = brk;
        }
    }

    fn pronunciation(&mut self, chars: &[char], i: usize) -> Result<usize, ParseError> {
        if self.text.is_empty() {
            return Err(error(
                i,
                "a pronunciation [...] follows the syllable it is for",
            ));
        }
        let Some(len) = chars[i + 1..].iter().position(|c| *c == ']') else {
            return Err(error(i, "a [ without its ]"));
        };
        let inside: String = chars[i + 1..i + 1 + len].iter().collect();
        self.phonemes = inside.split_whitespace().map(str::to_string).collect();
        if self.phonemes.is_empty() {
            return Err(error(i, "an empty pronunciation []"));
        }
        Ok(i + len + 2)
    }

    fn push(&mut self, sung: Sung) {
        self.tokens.push(Token {
            sung,
            brk: Break::None,
        });
    }

    fn end_syllable(&mut self) {
        if self.text.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.text);
        let phonemes = std::mem::take(&mut self.phonemes);
        self.word.push(self.tokens.len());
        self.push(Sung::Syllable {
            text,
            pos: WordPos::Single,
            phonemes,
        });
    }

    /// Give the word's syllables their places in it.
    fn end_word(&mut self) {
        self.cont = false;
        let n = self.word.len();
        for (k, &ix) in self.word.iter().enumerate() {
            if let Sung::Syllable { pos, .. } = &mut self.tokens[ix].sung {
                *pos = match (k, n) {
                    (_, 1) => WordPos::Single,
                    (0, _) => WordPos::Begin,
                    (k, n) if k + 1 == n => WordPos::End,
                    _ => WordPos::Middle,
                };
            }
        }
        if n > 0 {
            self.last_word = std::mem::take(&mut self.word);
        }
    }
}

fn starts_with(chars: &[char], i: usize, s: &str) -> bool {
    s.chars()
        .enumerate()
        .all(|(k, c)| chars.get(i + k) == Some(&c))
}

fn error(at: usize, message: &'static str) -> ParseError {
    ParseError { at, message }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokens as compact strings: `hel<` begin, `lo>` end, `ah` single,
    /// `x~` middle, `_` hold, `(br)`, then `/` or `//` for breaks.
    fn show(text: &str) -> Vec<String> {
        parse(text)
            .unwrap()
            .iter()
            .map(|t| {
                let mut s = match &t.sung {
                    Sung::Syllable {
                        text,
                        pos,
                        phonemes,
                    } => {
                        let mark = match pos {
                            WordPos::Single => "",
                            WordPos::Begin => "<",
                            WordPos::Middle => "~",
                            WordPos::End => ">",
                        };
                        let ph = if phonemes.is_empty() {
                            String::new()
                        } else {
                            format!("[{}]", phonemes.join(" "))
                        };
                        format!("{text}{ph}{mark}")
                    }
                    Sung::Hold => "_".into(),
                    Sung::Breath => "(br)".into(),
                };
                s += match t.brk {
                    Break::None => "",
                    Break::Line => "/",
                    Break::Paragraph => "//",
                };
                s
            })
            .collect()
    }

    #[derive(serde::Deserialize)]
    struct Case {
        text: String,
        tokens: Vec<String>,
        error: i64,
    }

    /// The cases web/src/lyrics.js is tested against too.
    #[test]
    fn shared_cases() {
        let js = include_str!("../../../../web/test/lyric-notation.js");
        let json = &js[js.find("= [").unwrap() + 2..js.rfind(']').unwrap() + 1];
        let cases: Vec<Case> = serde_json::from_str(json).unwrap();
        for c in cases {
            if c.error >= 0 {
                let e = parse(&c.text).unwrap_err();
                assert_eq!(e.at as i64, c.error, "{:?}", c.text);
            } else {
                assert_eq!(show(&c.text), c.tokens, "{:?}", c.text);
            }
        }
    }

    #[test]
    fn a_breath_can_be_written_as_text() {
        let t = parse(r"\(br)").unwrap();
        assert!(matches!(&t[0].sung, Sung::Syllable { text, .. } if text == "(br)"));
    }
}
