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

/// Wrap tracks (and an FX mixer) in an LMMS 1.2 song at 120 BPM.
fn song(head: &str, tracks: &str, mixer: &str) -> Vec<u8> {
    format!(
        r#"<?xml version="1.0"?><lmms-project creatorversion="1.2.1" type="song">{head}
      <song><trackcontainer type="song">{tracks}</trackcontainer>{mixer}</song></lmms-project>"#
    )
    .into_bytes()
}

const HEAD: &str = r#"<head bpm="120" mastervol="100" masterpitch="0" timesig_numerator="4" timesig_denominator="4"/>"#;

fn triple(name: &str, extra: &str, patterns: &str) -> String {
    format!(
        r#"<track type="0" name="{name}"><instrumenttrack basenote="57" fxch="0">
          <instrument name="tripleoscillator"><tripleoscillator vol0="50" vol1="0" vol2="0"/></instrument>{extra}</instrumenttrack>{patterns}</track>"#
    )
}

#[test]
fn lmms_1_2_patterns_span_their_notes() {
    // LMMS 1.2 saves no `len`: melody patterns span their notes in whole
    // bars, beat patterns their steps.
    let tracks = triple(
        "Lead",
        "",
        r#"<pattern type="1" steps="16" pos="0"><note key="57" pos="0" len="48"/><note key="57" pos="768" len="96"/></pattern>
           <pattern type="0" steps="32" pos="1920"><note key="57" pos="0" len="-192"/></pattern>
           <pattern type="0" steps="16" pos="3840"/>"#,
    );
    let im = lmms::import(&song(HEAD, &tracks, ""), &Options::new("t")).unwrap();
    let p = &im.project;
    let clips: Vec<(f64, f64)> = p
        .playlist
        .clips
        .iter()
        .map(|c| (c.start, c.length))
        .collect();
    // 768 + 96 ticks = 18 beats → 5 bars.
    assert_eq!(clips, [(0.0, 20.0), (40.0, 8.0), (80.0, 4.0)]);
    let lead = &p.patterns[0];
    assert_eq!(lead.length, 20.0);
    assert_eq!(lead.notes.len(), 2);
}

#[test]
fn automation_becomes_lanes() {
    let tracks = format!(
        r#"{}
        <track type="5" name="Automation track"><automationtrack/>
          <automationpattern name="Lead>Volume" pos="192" len="384" prog="1" mute="0">
            <time pos="0" value="100"/><time pos="192" value="50"/><time pos="576" value="0"/>
            <object id="11"/>
          </automationpattern>
          <automationpattern name="Lead>Volume" pos="960" len="192" prog="0" mute="0">
            <time pos="0" value="20"/><time pos="96" value="80"/>
            <object id="11"/>
          </automationpattern>
          <automationpattern name="Bus" pos="0" len="192" prog="1" mute="0">
            <time pos="0" value="1"/><time pos="192" value="0.5"/>
            <object id="22"/>
          </automationpattern>
          <automationpattern name="Cutoff" pos="0" len="192" prog="1" mute="0">
            <time pos="0" value="1"/><object id="33"/>
          </automationpattern>
        </track>"#,
        triple(
            "Lead",
            r#"<vol value="80" id="11"/><pan value="0" id="12"/>"#,
            r#"<pattern type="1" pos="0"><note key="57" pos="0" len="48"/></pattern>"#
        )
    );
    let mixer = r#"<fxmixer><fxchannel num="0" name="Master" volume="1" muted="0"/>
        <fxchannel num="1" name="Bus" muted="0"><volume value="1" id="22"/></fxchannel></fxmixer>"#;
    let head = r#"<head mastervol="100" masterpitch="0"><bpm value="120" id="44"/></head>"#;
    let tracks = format!(
        r#"{tracks}<track type="6" name="Automation track"><automationtrack/>
          <automationpattern name="Tempo" pos="384" len="192" prog="0"><time pos="0" value="90"/><object id="44"/></automationpattern>
        </track>"#
    );
    let im = lmms::import(&song(head, &tracks, mixer), &Options::new("t")).unwrap();
    let p = &im.project;
    let lane = |target: &str| {
        p.automation
            .iter()
            .find(|l| l.target == target)
            .unwrap_or_else(|| panic!("no lane for {target}: {:#?}", p.automation))
            .points
            .iter()
            .map(|q| (q.beat, q.value))
            .collect::<Vec<_>>()
    };
    let lead = &p.channels[0].id;
    assert_eq!(p.channels[0].volume, 0.8);
    // Saved value until the first pattern (beat 4); linear from 100% to
    // 50% over a bar, then toward 0 until the pattern ends at beat 12
    // (25%), held until the discrete pattern at beat 20, which steps.
    assert_eq!(
        lane(&format!("channel/{lead}/volume")),
        [
            (0.0, 0.8),
            (4.0, 0.8),
            (4.0, 1.0),
            (8.0, 0.5),
            (12.0, 0.25),
            (20.0, 0.25),
            (20.0, 0.2),
            (22.0, 0.2),
            (22.0, 0.8)
        ]
    );
    assert_eq!(lane("insert/1/volume"), [(0.0, 1.0), (4.0, 0.5)]);
    assert_eq!(lane("tempo"), [(0.0, 120.0), (8.0, 120.0), (8.0, 90.0)]);
    assert_eq!(p.automation[0].name, "Lead · Volume");
    assert!(
        im.warnings
            .iter()
            .any(|w| w.contains("\"Cutoff\"") && w.contains("cannot automate")),
        "{:#?}",
        im.warnings
    );
}

#[test]
fn arpeggios_play_on_the_channel_and_chords_are_written_out() {
    // 120 BPM: 250 ms = half a beat.
    let arp = r#"<arpeggiator arp-enabled="1" arp="3" arprange="2" arptime="250" arpgate="50" arpdir="2" arpmode="1"/>"#;
    let chord = r#"<chordcreator chord-enabled="1" chord="1" chordrange="1"/>"#;
    let note = r#"<pattern type="1" pos="0"><note key="57" pos="0" len="96"/></pattern>"#;
    let tracks = format!(
        "{}{}",
        triple("Arp", arp, note),
        triple("Chord", chord, note)
    );
    let im = lmms::import(&song(HEAD, &tracks, ""), &Options::new("t")).unwrap();
    let p = &im.project;
    let notes = |ch: &str| {
        p.patterns
            .iter()
            .flat_map(|x| &x.notes)
            .filter(|n| n.channel == ch)
            .map(|n| (n.pitch, n.start, n.length))
            .collect::<Vec<_>>()
    };
    // The note stays as written; the channel arpeggiates it as LMMS did.
    assert_eq!(notes(&p.channels[0].id), [(69, 0.0, 2.0)]);
    assert_eq!(
        p.channels[0].arp,
        Some(rosaclef_core::Arpeggio {
            chord: "minor".into(),
            octaves: 2,
            rate: 0.5,
            direction: "updown".into(),
            gate: 0.5,
            mode: "sort".into(),
        })
    );
    assert_eq!(p.channels[1].arp, None);
    assert_eq!(
        notes(&p.channels[1].id),
        [(69, 0.0, 2.0), (73, 0.0, 2.0), (76, 0.0, 2.0)]
    );
}

#[test]
fn maps_soundfonts_drumsynth_and_plugin_effects() {
    let reverb = r#"<fxchain numofeffects="1" enabled="1"><effect name="ladspaeffect" on="1" wet="1">
        <ladspacontrols/><key><attribute name="file" value="calf"/><attribute name="plugin" value="Reverb"/></key>
      </effect></fxchain>"#;
    let tracks = format!(
        r#"<track type="0" name="Violin"><instrumenttrack basenote="57" vol="100" fxch="1" pitch="-100">
          <instrument name="sf2player"><sf2player src="soundfonts/FatBoy.sf2" bank="0" patch="40" gain="1.5"/></instrument>{reverb}</instrumenttrack>
          <pattern type="1" pos="0"><note key="57" pos="0" len="48"/></pattern></track>
        <track type="0" name="Hat"><instrumenttrack basenote="57" vol="100" fxch="2">
          <instrument name="audiofileprocessor"><audiofileprocessor src="drumsynth/tr808/Hat_c.ds" amp="100"/></instrument>{reverb}</instrumenttrack>
          <pattern type="0" steps="16" pos="0"><note key="57" pos="0" len="-192"/></pattern></track>
        <track type="0" name="Kick"><instrumenttrack basenote="57" vol="100" fxch="2">
          <instrument name="audiofileprocessor"><audiofileprocessor src="drumsynth/tr808/Kickhard.ds" amp="100"/></instrument></instrumenttrack></track>"#
    );
    let mixer = r#"<fxmixer><fxchannel num="0" name="Master" volume="1"/>
        <fxchannel num="1" name="Strings" volume="0.5"/>
        <fxchannel num="2" name="Drums" volume="0.4" muted="0"/></fxmixer>"#;
    let im = lmms::import(&song(HEAD, &tracks, mixer), &Options::new("t")).unwrap();
    let p = &im.project;
    let violin = &p.channels[0];
    assert_eq!(violin.instrument.kind, "soundfont");
    assert_eq!(violin.instrument.option("program"), "Violin");
    assert_eq!(violin.instrument.param("gain"), 1.5);
    // The pitch knob (-100 cents) transposes down a semitone.
    assert_eq!(p.patterns[0].notes[0].pitch, 68);
    // Strings serves only the violin: its reverb stays on that insert.
    assert_eq!(violin.mixer.0, 1);
    assert_eq!(p.mixer.inserts[1].effects[0].kind, "reverb");
    // The Calf reverb keeps its dry at full level: the insert makes up
    // for the crossfade of the built-in reverb.
    let strings = &p.mixer.inserts[1];
    let mix = strings.effects[0].param("mix");
    assert!((strings.volume - 0.5 / (1.0 - mix / 2.0)).abs() < 1e-9);
    let (hat, kick) = (&p.channels[1], &p.channels[2]);
    assert_eq!(hat.instrument.kind, "drum");
    assert_eq!(hat.instrument.option("kind"), "hat");
    assert_eq!(kick.instrument.option("kind"), "kick");
    assert!(
        im.samples.is_empty(),
        "DrumSynth patches are not sample files"
    );
    // Drums is shared: the hat's reverb gets a copy of it.
    assert_eq!(kick.mixer.0, 2);
    let own = &p.mixer.inserts[hat.mixer.index()];
    assert_ne!(hat.mixer.0, 2);
    assert_eq!(own.effects[0].kind, "reverb");
    let mix = own.effects[0].param("mix");
    assert!(
        (own.volume - 0.4 / (1.0 - mix / 2.0)).abs() < 1e-9,
        "Drums' fader, dry kept"
    );
}

#[test]
fn envelopes_and_bass_booster_follow_lmms() {
    // LMMS 1.2 saves `sustain` (amplitude = its square) and a hold stage;
    // the bass booster is (in + lowpass · ratio) · gain.
    let el = r#"<eldata fwet="0"><elvol amt="1" att="0" hold="0.5" dec="0.2" sustain="0.5" rel="0.2"/></eldata>
        <fxchain numofeffects="1" enabled="1"><effect name="bassbooster" on="1" wet="1">
          <bassboostercontrols gain="2" ratio="3" freq="100"/></effect></fxchain>"#;
    let tracks = triple(
        "Bass",
        el,
        r#"<pattern type="1" pos="0"><note key="45" pos="0" len="48"/></pattern>"#,
    )
    .replace(r#"fxch="0""#, r#"fxch="1""#);
    let mixer = r#"<fxmixer><fxchannel num="0" name="Master" volume="1"/><fxchannel num="1" name="Bass" volume="0.5"/></fxmixer>"#;
    let im = lmms::import(&song(HEAD, &tracks, mixer), &Options::new("t")).unwrap();
    let p = &im.project;
    let synth = &p.channels[0].instrument;
    assert_eq!(synth.param("sustain"), 0.25);
    // hold 5·0.5² + decay 5·0.2²
    assert!((synth.param("decay") - 1.45).abs() < 1e-9);
    let bus = &p.mixer.inserts[1];
    assert_eq!(
        p.channels[0].mixer.0, 1,
        "the only user keeps its FX channel"
    );
    assert_eq!(bus.effects[0].kind, "eq");
    assert!(
        (bus.effects[0].param("low") - 12.041).abs() < 0.01,
        "20·log10(1 + 3)"
    );
    assert!((bus.effects[0].param("lowFreq") - 69.84).abs() < 0.1);
    assert_eq!(bus.volume, 1.0, "FX volume 0.5 × booster gain 2");
}

#[test]
fn ladspa_effects_read_their_ports() {
    // TAP Reverberator: decay 1175 ms, dry 0 dB, wet 0 dB, band-pass on.
    // Calf Phaser: base 632 Hz, depth 6210 cents, 0.1 Hz, 6 stages, 180°,
    // amount 1 and dry 1 (dry + effect).
    let fx = r#"<fxchain numofeffects="2" enabled="1">
        <effect name="ladspaeffect" on="1" wet="1"><ladspacontrols ports="8"><port00 data="1175"/><port01 data="0"/><port02 data="0"/><port05 data="1"/></ladspacontrols>
          <key><attribute name="file" value="tap_reverb"/><attribute name="plugin" value="tap_reverb"/></key></effect>
        <effect name="ladspaeffect" on="1" wet="1"><ladspacontrols ports="9"><port04 data="631.867"/><port05 data="6210"/><port06 data="0.1"/><port07 data="0"/><port08 data="6"/><port09 data="180"/><port011 data="1"/><port012 data="1"/></ladspacontrols>
          <key><attribute name="file" value="calf"/><attribute name="plugin" value="Phaser"/></key></effect>
      </fxchain>"#;
    let tracks = triple(
        "Pad",
        fx,
        r#"<pattern type="1" pos="0"><note key="57" pos="0" len="48"/></pattern>"#,
    )
    .replace(r#"fxch="0""#, r#"fxch="1""#);
    let mixer = r#"<fxmixer><fxchannel num="0" name="Master" volume="1"/><fxchannel num="1" name="Pad" volume="1"/></fxmixer>"#;
    let im = lmms::import(&song(HEAD, &tracks, mixer), &Options::new("t")).unwrap();
    let ins = &im.project.mixer.inserts[1];
    let kinds: Vec<&str> = ins.effects.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, ["eq", "reverb", "phaser"]);
    let (eq, rev, ph) = (&ins.effects[0], &ins.effects[1], &ins.effects[2]);
    // Its band-pass wet path lifts the upper mids and highs by 6 dB (wet =
    // dry), and
    // leaves only a light tail: fast notes stay crisp.
    for band in ["mid", "high"] {
        assert!(
            (eq.param(band) - 6.02).abs() < 0.01,
            "{band} {}",
            eq.param(band)
        );
    }
    assert_eq!(
        (eq.param("midFreq"), eq.param("highFreq")),
        (1700.0, 4000.0)
    );
    assert!(
        (0.4..0.6).contains(&rev.param("size")),
        "{}",
        rev.param("size")
    );
    assert_eq!(rev.param("mix"), 0.25);
    // The phaser keeps the Calf settings; equal dry and phased: mix 1.
    assert_eq!(ph.param("mix"), 1.0);
    assert!((ph.param("freq") - 631.867).abs() < 1e-9);
    assert!((ph.param("depth") - 6210.0 / 7200.0).abs() < 1e-9);
    assert_eq!(
        (ph.param("rate"), ph.param("stages"), ph.param("stereo")),
        (0.1, 6.0, 0.5)
    );
    // Both keep the dry at full level; the insert restores it.
    let expect: f64 = 1.0 / (1.0 - 0.25 / 2.0) * 2.0;
    assert!(
        (ins.volume - expect.min(2.0)).abs() < 1e-9,
        "{} vs {expect}",
        ins.volume
    );
}
