//! Factory presets for the built-in instruments.
//!
//! Each engine has its own file. A preset is a device type plus the params
//! and options that differ from the catalog defaults; `device()` turns it
//! into a project `Device`. Every preset is validated against the catalog
//! by the tests below.

mod classic;
mod comete;
mod cuivre;
mod dedale;
mod nebula;
mod prisme;
mod sextant;
mod tessera;

use crate::Device;
use serde::ser::{Serialize, SerializeMap, SerializeStruct, Serializer};

#[derive(Clone, Copy, Debug)]
pub struct Preset {
    pub name: &'static str,
    /// Device type (`"prisme"`, `"sextant"`, ...).
    pub kind: &'static str,
    /// Comma-separated tags: role and character ("pad, cinematic").
    pub tags: &'static str,
    pub doc: &'static str,
    pub params: &'static [(&'static str, f64)],
    pub options: &'static [(&'static str, &'static str)],
}

impl Preset {
    pub fn device(&self) -> Device {
        let mut d = Device::new(self.kind);
        for (k, v) in self.params {
            d.params.insert(k.to_string(), *v);
        }
        for (k, v) in self.options {
            d.options.insert(k.to_string(), v.to_string());
        }
        d
    }
}

struct Pairs<T: 'static>(&'static [(&'static str, T)]);

impl<T: Serialize> Serialize for Pairs<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.0.len()))?;
        for (k, v) in self.0 {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl Serialize for Preset {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut st = s.serialize_struct("Preset", 6)?;
        st.serialize_field("name", self.name)?;
        st.serialize_field("type", self.kind)?;
        st.serialize_field("tags", self.tags)?;
        st.serialize_field("doc", self.doc)?;
        st.serialize_field("params", &Pairs(self.params))?;
        st.serialize_field("options", &Pairs(self.options))?;
        st.end()
    }
}

/// Every factory preset, grouped by engine.
pub fn all() -> Vec<&'static Preset> {
    [
        classic::PRESETS,
        prisme::PRESETS,
        sextant::PRESETS,
        tessera::PRESETS,
        cuivre::PRESETS,
        nebula::PRESETS,
        dedale::PRESETS,
        comete::PRESETS,
    ]
    .iter()
    .flat_map(|list| list.iter())
    .collect()
}

pub fn find(name: &str) -> Option<&'static Preset> {
    all()
        .into_iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{validate, Channel, InsertIx, Project};

    #[test]
    fn every_preset_is_valid() {
        let mut p = Project::empty("presets");
        for (i, preset) in all().iter().enumerate() {
            p.channels.push(Channel {
                id: format!("p{i}"),
                name: preset.name.into(),
                color: "#ffffff".into(),
                instrument: preset.device(),
                volume: 0.8,
                pan: 0.0,
                mute: false,
                mixer: InsertIx::MASTER,
                arp: None,
            });
        }
        let issues = validate::validate(&p);
        let errors: Vec<String> = issues
            .iter()
            .filter(|i| i.severity == validate::Severity::Error)
            .map(|i| {
                let idx: usize = i
                    .path
                    .trim_start_matches("channels[")
                    .split(']')
                    .next()
                    .unwrap_or("0")
                    .parse()
                    .unwrap_or(0);
                format!("{} ({}): {}", all()[idx].name, i.path, i.message)
            })
            .collect();
        assert!(errors.is_empty(), "invalid presets:\n{}", errors.join("\n"));
    }

    #[test]
    fn preset_names_are_unique() {
        let names: Vec<String> = all().iter().map(|p| p.name.to_lowercase()).collect();
        for (i, n) in names.iter().enumerate() {
            assert!(!names[i + 1..].contains(n), "duplicate preset name {n}");
        }
    }
}
