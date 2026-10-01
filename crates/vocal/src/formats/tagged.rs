//! Lyrics tagged by section, the text song generators take (ACE-Step, YuE,
//! LeVo, SongBloom, Suno): a `[Verse 1]`, `[Chorus]`... line, then the
//! paragraph's lyric lines; a blank line between paragraphs.
//!
//! A paragraph is named after the pattern it is sung in when that name
//! names a section ("Chorus", "Pre-chorus 2"), numbered by verse when the
//! song sings it with several verses. Otherwise paragraphs are `Verse 1`,
//! `Verse 2`... in order of first appearance; a paragraph sung again with
//! the same words keeps its name.

use super::paragraphs;
use crate::line::Song;
use crate::words::Phrase;
use std::collections::HashMap;

pub fn write(song: &Song) -> String {
    let paras = paragraphs(song);
    let keys: Vec<(&str, u32)> = paras.iter().map(|p| p[0].section()).collect();
    let names = Names::new(&keys);
    let blocks: Vec<String> = paras
        .iter()
        .zip(&keys)
        .map(|(p, k)| block(&names.of(*k), p))
        .collect();
    blocks.join("\n")
}

fn block(name: &str, phrases: &[Phrase]) -> String {
    let mut s = format!("[{name}]\n");
    for ph in phrases {
        s += &ph.text();
        s.push('\n');
    }
    s
}

/// Words that name a song section, as a pattern name starts.
const SECTIONS: &[&str] = &[
    "intro",
    "verse",
    "pre-chorus",
    "prechorus",
    "chorus",
    "post-chorus",
    "refrain",
    "hook",
    "bridge",
    "breakdown",
    "break",
    "interlude",
    "instrumental",
    "solo",
    "drop",
    "build",
    "outro",
    "coda",
];

/// The name of each (section, verse) a paragraph is sung in.
struct Names {
    of: HashMap<(String, u32), String>,
}

impl Names {
    fn new(keys: &[(&str, u32)]) -> Names {
        let mut of = HashMap::new();
        let mut unnamed = 0;
        for &(section, verse) in keys {
            let key = (section.to_string(), verse);
            if of.contains_key(&key) {
                continue;
            }
            let name = if names_a_section(section) {
                let several = keys.iter().any(|k| k.0 == section && k.1 != verse);
                if several && !section.chars().any(|c| c.is_ascii_digit()) {
                    format!("{section} {verse}")
                } else {
                    section.to_string()
                }
            } else {
                unnamed += 1;
                format!("Verse {unnamed}")
            };
            of.insert(key, name);
        }
        Names { of }
    }

    fn of(&self, (section, verse): (&str, u32)) -> String {
        self.of[&(section.to_string(), verse)].clone()
    }
}

fn names_a_section(name: &str) -> bool {
    let first: String = name
        .trim()
        .split(|c: char| c.is_whitespace() || c.is_ascii_digit())
        .next()
        .unwrap_or("")
        .to_lowercase();
    SECTIONS.contains(&first.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{project, song};

    #[test]
    fn paragraphs_named_by_their_patterns() {
        assert_eq!(
            write(&song()),
            "[Verse 1]\nHello dark friend\nHi\n\n[Hook]\nLa la\n\n\
             [Verse 2]\nBye now my friend\nHi\n\n[Hook]\nLa la\n"
        );
    }

    #[test]
    fn other_names_count_verses() {
        let mut p = project();
        p.patterns[0].name = "Lead A".into();
        p.patterns[1].name = "Riff".into();
        let text = write(&Song::of_project(&p));
        let tags: Vec<&str> = text.lines().filter(|l| l.starts_with('[')).collect();
        assert_eq!(tags, ["[Verse 1]", "[Verse 2]", "[Verse 3]", "[Verse 2]"]);
        assert!(names_a_section("Pre-chorus 2") && !names_a_section("Versatile"));
    }
}
