//! LMMS import tests with small hand-written projects.

use rosaclef_core::validate;
use rosaclef_import::lmms::{self, Options};
use rosaclef_import::Imported;
use std::io::Write;
use std::path::PathBuf;

const SONG: &str = r##"<?xml version="1.0"?>
<!DOCTYPE lmms-project>
<lmms-project version="1.0" creator="LMMS" creatorversion="1.2.2" type="song">
  <head timesig_numerator="4" mastervol="100" timesig_denominator="4" bpm="128" masterpitch="0"/>
  <song>
    <trackcontainer width="600" x="5" y="5" maximized="0" height="300" visible="1" type="song" minimized="0">
      <track muted="0" type="0" name="Lead" solo="0">
        <instrumenttrack pan="-50" fxch="1" pitchrange="1" pitch="0" basenote="57" vol="100">
          <instrument name="tripleoscillator">
            <tripleoscillator vol0="33" wavetype0="2" coarse0="0" finel0="0" finer0="0"
                              vol1="33" wavetype1="3" coarse1="-12" finel1="5" finer1="5"
                              vol2="0" wavetype2="0" coarse2="-24" finel2="0" finer2="0"
                              modalgo1="2" modalgo2="2"/>
          </instrument>
          <eldata fres="0.5" ftype="0" fcut="14000" fwet="0">
            <elvol amt="1" att="0.1" dec="0.5" sus="0.25" rel="0.2" lshp="0"/>
          </eldata>
          <fxchain numofeffects="0" enabled="0"/>
        </instrumenttrack>
        <pattern muted="0" steps="16" type="1" name="" pos="0" len="192" frozen="0">
          <note pan="0" key="57" vol="100" pos="0" len="48"/>
          <note pan="0" key="60" vol="50" pos="96" len="24"/>
        </pattern>
        <pattern muted="0" steps="16" type="1" name="" pos="384" len="192" frozen="0">
          <note pan="0" key="57" vol="100" pos="0" len="48"/>
          <note pan="0" key="60" vol="50" pos="96" len="24"/>
        </pattern>
        <pattern muted="0" steps="32" type="1" name="Break" pos="768" len="384" frozen="0">
          <note pan="0" key="48" vol="75" pos="192" len="96"/>
        </pattern>
      </track>
      <track muted="0" type="1" name="Beat/Bassline 0" solo="0">
        <bbtrack>
          <trackcontainer width="640" type="bbtrackcontainer" visible="1">
            <track muted="0" type="0" name="Kicker" solo="0">
              <instrumenttrack pan="0" fxch="2" basenote="57" vol="100">
                <instrument name="kicker">
                  <kicker startfreq="150" endfreq="40" decay="440" dist="0.8" gain="1" click="0.4" noise="0" startnote="0" endnote="0"/>
                </instrument>
              </instrumenttrack>
              <pattern muted="0" steps="16" type="0" name="" pos="0" len="192" frozen="0">
                <note pan="0" key="57" vol="100" pos="0" len="-192"/>
                <note pan="0" key="57" vol="100" pos="48" len="-192"/>
                <note pan="0" key="57" vol="100" pos="96" len="-192"/>
                <note pan="0" key="57" vol="100" pos="144" len="-192"/>
              </pattern>
              <pattern muted="0" steps="16" type="0" name="" pos="192" len="192" frozen="0">
                <note pan="0" key="57" vol="100" pos="0" len="-192"/>
              </pattern>
            </track>
            <track muted="1" type="0" name="Hat Sample" solo="0">
              <instrumenttrack pan="0" fxch="2" basenote="57" vol="80">
                <instrument name="audiofileprocessor">
                  <audiofileprocessor src="hat.wav" sframe="0" eframe="1" looped="0" amp="100" reversed="0" stutter="0" interp="1"/>
                </instrument>
              </instrumenttrack>
              <pattern muted="0" steps="16" type="0" name="" pos="0" len="192" frozen="0">
                <note pan="0" key="57" vol="120" pos="24" len="-192"/>
              </pattern>
              <pattern muted="0" steps="16" type="0" name="" pos="192" len="192" frozen="0"/>
            </track>
          </trackcontainer>
        </bbtrack>
        <bbtco muted="0" pos="0" len="384" name="" usestyle="1"/>
        <bbtco muted="0" pos="768" len="192" name="" usestyle="1"/>
      </track>
      <track muted="0" type="1" name="Fill" solo="0">
        <bbtrack/>
        <bbtco muted="0" pos="1152" len="192" name="" usestyle="1"/>
      </track>
      <track muted="1" type="2" name="Vocals" solo="0">
        <sampletrack vol="50" pan="0" fxch="3">
          <fxchain numofeffects="0" enabled="0"/>
        </sampletrack>
        <sampletco muted="0" pos="192" len="768" src="vox.wav" off="-48"/>
        <sampletco muted="0" pos="1152" len="192" src="missing.wav" off="0"/>
      </track>
      <track muted="0" type="0" name="Pad" solo="0">
        <instrumenttrack pan="0" fxch="0" basenote="57" vol="100">
          <instrument name="zynaddsubfx"><zynaddsubfx/></instrument>
          <fxchain numofeffects="1" enabled="1">
            <effect name="reverbsc" on="1" wet="0.4" autoquit="1" gate="0">
              <reverbsc size="0.7" color="8000" input_gain="0" output_gain="0"/>
            </effect>
          </fxchain>
        </instrumenttrack>
        <pattern muted="0" steps="16" type="1" name="" pos="0" len="192" frozen="0">
          <note pan="0" key="69" vol="100" pos="0" len="192"/>
        </pattern>
      </track>
      <track muted="0" type="0" name="Acid" solo="0">
        <instrumenttrack pan="0" fxch="9" basenote="57" vol="100">
          <instrument name="lb302">
            <lb302 vcf_cut="0.6" vcf_res="0.9" vcf_mod="0.5" vcf_dec="0.2" dist="0.3" shape="0" slide="1" slide_dec="0.5" db24="1"/>
          </instrument>
        </instrumenttrack>
        <pattern muted="0" steps="16" type="1" name="" pos="0" len="192" frozen="0">
          <note pan="0" key="33" vol="100" pos="0" len="12"/>
        </pattern>
      </track>
      <track muted="0" type="5" name="Automation track" solo="0">
        <automationtrack/>
      </track>
    </trackcontainer>
    <fxmixer width="543" x="5" y="310" maximized="0" height="333" visible="1" minimized="0">
      <fxchannel num="0" muted="0" volume="1" name="Master">
        <fxchain numofeffects="0" enabled="0"/>
      </fxchannel>
      <fxchannel num="1" muted="0" volume="0.8" name="Lead Bus">
        <fxchain numofeffects="1" enabled="1">
          <effect name="delay" on="1" wet="1" autoquit="1" gate="0">
            <Delay DelayTimeSamples="0.25" FeebackAmount="0.4" LfoFrequency="2" LfoAmount="0" OutGain="0"/>
          </effect>
        </fxchain>
        <send channel="0" amount="1"/>
      </fxchannel>
      <fxchannel num="2" muted="0" volume="1.2" name="Drums">
        <fxchain numofeffects="0" enabled="0"/>
        <send channel="1" amount="1"/>
      </fxchannel>
      <fxchannel num="3" muted="1" volume="1" name="Vox">
        <fxchain numofeffects="0" enabled="0"/>
        <send channel="0" amount="1"/>
      </fxchannel>
    </fxmixer>
  </song>
</lmms-project>
"##;

/// A folder next to the .mmp with the samples it references.
fn source_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rosaclef-lmms-test-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("hat.wav"), b"RIFF-not-really").unwrap();
    std::fs::write(dir.join("vox.wav"), b"RIFF-not-really").unwrap();
    dir
}

fn import(bytes: &[u8], tag: &str) -> Imported {
    let mut opts = Options::new("Imported Song");
    opts.source_dir = Some(source_dir(tag));
    opts.sample_dirs = vec![];
    lmms::import(bytes, &opts).expect("import")
}

fn has_warning(im: &Imported, needle: &str) -> bool {
    im.warnings.iter().any(|w| w.contains(needle))
}

#[test]
fn imports_a_song() {
    let im = import(SONG.as_bytes(), "song");
    let p = &im.project;
    let checked = validate::validate(p);
    assert!(
        checked
            .iter()
            .all(|i| i.severity != validate::Severity::Error),
        "{checked:?}"
    );

    // Transport.
    assert_eq!(p.transport.bpm, 128.0);
    assert_eq!(p.transport.beats_per_bar, 4);
    assert_eq!(p.meta.title, "Imported Song");

    // Channels, in track order (B&B channels where the first B&B appears).
    let names: Vec<&str> = p.channels.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Lead", "Kicker", "Hat Sample", "Pad", "Acid"]);
    let lead = &p.channels[0];
    assert_eq!(lead.instrument.kind, "synth");
    assert_eq!(lead.instrument.option("wave1"), "saw");
    assert_eq!(lead.instrument.option("wave2"), "square");
    assert_eq!(lead.instrument.param("osc2Semi"), -12.0);
    assert_eq!(lead.instrument.param("sustain"), 0.75);
    assert!((lead.instrument.param("attack") - 0.05).abs() < 1e-9);
    assert_eq!(lead.pan, -0.5);
    assert_eq!(lead.mixer.0, 1);
    let kick = &p.channels[1];
    assert_eq!(kick.instrument.kind, "drum");
    assert_eq!(kick.instrument.option("kind"), "kick");
    assert_eq!(kick.mixer.0, 2);
    let hat = &p.channels[2];
    assert_eq!(hat.instrument.kind, "sampler");
    assert_eq!(hat.instrument.option("sample"), "samples/hat.wav");
    assert_eq!(hat.instrument.option("mode"), "oneshot");
    assert_eq!(hat.instrument.param("root"), 69.0);
    assert!(hat.mute, "muted B&B tracks mute their channel");
    assert_eq!(hat.volume, 0.8);
    let pad = &p.channels[3];
    assert_eq!(pad.instrument.kind, "synth");
    let acid = &p.channels[4];
    assert_eq!(
        acid.instrument.kind, "cuivre",
        "LB302 becomes the Cuivre virtual analog"
    );
    assert_eq!(acid.instrument.option("wave1"), "saw");
    assert_eq!(
        acid.instrument.option("filter"),
        "ladder",
        "db24 selects the 4-pole ladder"
    );
    assert_eq!(
        acid.instrument.option("mode"),
        "legato",
        "LB302 slide plays legato"
    );
    assert!(acid.instrument.param("resonance") > 0.5);
    assert!(
        acid.instrument.param("glide") > 0.01,
        "LB302 slide becomes glide"
    );
    assert_eq!(
        acid.mixer.0, 0,
        "routing to a missing FX channel falls back to the master"
    );

    // Pitches: LMMS key 57 = A4 = MIDI 69.
    let lead_patterns: Vec<_> = p
        .patterns
        .iter()
        .filter(|x| x.notes.iter().any(|n| n.channel == lead.id))
        .collect();
    assert_eq!(
        lead_patterns.len(),
        2,
        "identical LMMS patterns are deduplicated"
    );
    let a = lead_patterns[0];
    assert_eq!(a.length, 4.0);
    assert_eq!(
        (
            a.notes[0].pitch,
            a.notes[0].start,
            a.notes[0].length,
            a.notes[0].velocity
        ),
        (69, 0.0, 1.0, 1.0)
    );
    assert_eq!(
        (
            a.notes[1].pitch,
            a.notes[1].start,
            a.notes[1].length,
            a.notes[1].velocity
        ),
        (72, 2.0, 0.5, 0.5)
    );
    let b = lead_patterns[1];
    assert_eq!(b.name, "Break");
    assert_eq!(b.length, 8.0);
    assert_eq!(
        (b.notes[0].pitch, b.notes[0].start, b.notes[0].length),
        (60, 4.0, 2.0)
    );
    assert_eq!(p.channels[4].id, acid.id);
    let acid_note = p
        .patterns
        .iter()
        .flat_map(|x| &x.notes)
        .find(|n| n.channel == acid.id)
        .unwrap();
    assert_eq!(acid_note.pitch, 45);
    assert_eq!(acid_note.length, 0.25);

    // Playlist tracks mirror LMMS tracks (plus padding).
    let tracks: Vec<(&str, bool)> = p
        .playlist
        .tracks
        .iter()
        .map(|t| (t.name.as_str(), t.mute))
        .collect();
    assert_eq!(
        &tracks[..6],
        &[
            ("Lead", false),
            ("Beat/Bassline 0", false),
            ("Fill", false),
            ("Vocals", true),
            ("Pad", false),
            ("Acid", false)
        ]
    );
    let clips_on = |t: u32| {
        p.playlist
            .clips
            .iter()
            .filter(move |c| c.track.0 == t)
            .collect::<Vec<_>>()
    };
    let lead_clips: Vec<(f64, f64, &str)> = clips_on(0)
        .iter()
        .map(|c| (c.start, c.length, c.pattern.as_str()))
        .collect();
    assert_eq!(
        lead_clips,
        [
            (0.0, 4.0, a.id.as_str()),
            (8.0, 4.0, a.id.as_str()),
            (16.0, 8.0, b.id.as_str())
        ]
    );

    // Beat+Bassline: one multi-channel pattern per B&B, clips where bbtcos are.
    let bb0 = p
        .patterns
        .iter()
        .find(|x| x.name == "Beat/Bassline 0")
        .unwrap();
    assert_eq!(bb0.length, 4.0);
    let kicks: Vec<(i32, f64, f64)> = bb0
        .notes
        .iter()
        .filter(|n| n.channel == kick.id)
        .map(|n| (n.pitch, n.start, n.length))
        .collect();
    assert_eq!(
        kicks,
        [
            (60, 0.0, 0.25),
            (60, 1.0, 0.25),
            (60, 2.0, 0.25),
            (60, 3.0, 0.25)
        ]
    );
    let hats: Vec<(i32, f64, f64)> = bb0
        .notes
        .iter()
        .filter(|n| n.channel == hat.id)
        .map(|n| (n.pitch, n.start, n.velocity))
        .collect();
    assert_eq!(hats, [(69, 0.5, 1.0)]);
    let fill = p.patterns.iter().find(|x| x.name == "Fill").unwrap();
    assert_eq!(fill.notes.len(), 1);
    let bb_clips: Vec<(f64, f64, &str)> = clips_on(1)
        .iter()
        .map(|c| (c.start, c.length, c.pattern.as_str()))
        .collect();
    assert_eq!(
        bb_clips,
        [(0.0, 8.0, bb0.id.as_str()), (16.0, 4.0, bb0.id.as_str())]
    );
    let fill_clips: Vec<(f64, f64, &str)> = clips_on(2)
        .iter()
        .map(|c| (c.start, c.length, c.pattern.as_str()))
        .collect();
    assert_eq!(fill_clips, [(24.0, 4.0, fill.id.as_str())]);

    // Sample track: audio clips, copied samples, missing file warning.
    let audio = clips_on(3);
    assert_eq!(audio.len(), 2);
    assert_eq!(
        (
            audio[0].sample.as_str(),
            audio[0].start,
            audio[0].length,
            audio[0].offset,
            audio[0].gain,
            audio[0].mixer.0
        ),
        ("samples/vox.wav", 4.0, 16.0, 1.0, 0.5, 3)
    );
    assert_eq!(audio[1].sample, "samples/missing.wav");
    let copies: Vec<(&str, String)> = im
        .samples
        .iter()
        .map(|s| {
            (
                s.to.as_str(),
                s.from.file_name().unwrap().to_string_lossy().to_string(),
            )
        })
        .collect();
    assert_eq!(
        copies,
        [
            ("samples/hat.wav", "hat.wav".to_string()),
            ("samples/vox.wav", "vox.wav".to_string())
        ]
    );

    // Mixer: FX channels become inserts; track effects get their own insert.
    let ins: Vec<(&str, f64, bool)> = p
        .mixer
        .inserts
        .iter()
        .map(|i| (i.name.as_str(), i.volume, i.mute))
        .collect();
    assert_eq!(
        &ins[..4],
        &[
            ("Master", 1.0, false),
            ("Lead Bus", 0.8, false),
            ("Drums", 1.2, false),
            ("Vox", 1.0, true)
        ]
    );
    assert_eq!(p.mixer.inserts[1].effects[0].kind, "delay");
    assert!((p.mixer.inserts[1].effects[0].param("time") - 0.25 * 128.0 / 60.0).abs() < 1e-9);
    assert_eq!(p.mixer.inserts[0].effects.last().unwrap().kind, "limiter");
    assert_eq!(pad.mixer.0, 4);
    assert_eq!(p.mixer.inserts[4].name, "Pad FX");
    assert_eq!(p.mixer.inserts[4].effects[0].kind, "reverb");
    assert_eq!(p.mixer.inserts[4].effects[0].param("mix"), 0.4);

    // Everything approximated or skipped is reported.
    for needle in [
        "zynaddsubfx",
        "missing.wav",
        "FX 9",
        "from FX 2 to FX 1",
        "automation",
        "louder than 100%",
        "Kicker was approximated",
        "LB302",
        "reverbsc",
    ] {
        assert!(
            has_warning(&im, needle),
            "no warning mentioning {needle:?} in {:#?}",
            im.warnings
        );
    }
}

#[test]
fn imports_mmpz() {
    // Qt qCompress framing: 4-byte big-endian length, then a zlib stream.
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    enc.write_all(SONG.as_bytes()).unwrap();
    let mut bytes = (SONG.len() as u32).to_be_bytes().to_vec();
    bytes.extend(enc.finish().unwrap());
    let a = import(&bytes, "mmpz");
    let b = import(SONG.as_bytes(), "mmp");
    assert_eq!(a.project, b.project);
    assert_eq!(lmms::decode(&lmms::encode_mmpz(SONG)).unwrap(), SONG);
}

#[test]
fn transposes_by_base_note_and_master_pitch() {
    let xml = r#"<?xml version="1.0"?><lmms-project creatorversion="1.2.2"><head bpm="90" masterpitch="2" timesig_numerator="3" timesig_denominator="4"/>
      <song><trackcontainer type="song">
        <track type="0" name="Keys"><instrumenttrack basenote="45" vol="100" fxch="0"><instrument name="tripleoscillator"><tripleoscillator vol0="100" coarse0="12" vol1="0" vol2="0"/></instrument></instrumenttrack>
          <pattern pos="144" len="144"><note key="57" vol="100" pos="0" len="48"/></pattern></track>
      </trackcontainer></song></lmms-project>"#;
    let im = lmms::import(xml.as_bytes(), &Options::new("t")).unwrap();
    let p = &im.project;
    assert_eq!(p.transport.bpm, 90.0);
    assert_eq!(p.transport.beats_per_bar, 3);
    // 57 + 12 + (57 − 45) + masterpitch 2 + osc coarse 12
    assert_eq!(p.patterns[0].notes[0].pitch, 57 + 12 + 12 + 2 + 12);
    assert_eq!(p.playlist.clips[0].start, 3.0);
    assert_eq!(p.patterns[0].length, 3.0);
}

#[test]
fn rejects_non_lmms_files() {
    assert!(lmms::import(b"<html></html>", &Options::new("x")).is_err());
    assert!(lmms::import(b"\x00\x00\x01\x00not zlib at all", &Options::new("x")).is_err());
}
