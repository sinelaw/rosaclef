//! Native real-time audio: the engine driven by the system audio device
//! (cpal), plus recording from the default input device.
//!
//! The native engine runs alongside the browser's WebAssembly engine; the UI
//! chooses which one it listens to. Native playback is required for CLAP
//! plugins and gives the lowest latency.

use crate::folder::{self, Folder};
use crate::server::App;
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use rosaclef_core::{Clip, InsertIx, Project, TrackIx};
use rosaclef_engine::{Engine, PlayMode};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

struct Recording {
    active: AtomicBool,
    buf: Mutex<Vec<f32>>,
    channels: Mutex<u16>,
    sample_rate: Mutex<u32>,
}

pub struct Native {
    engine: Arc<Mutex<Engine>>,
    stop: std::sync::mpsc::Sender<()>,
    device: String,
    input: Option<String>,
    sample_rate: u32,
    rec: Arc<Recording>,
    /// Song position (beats) and track where the current take started.
    rec_start: Mutex<Option<(f64, u32)>>,
}

impl Native {
    pub fn start(project: Project, folder: &Folder) -> Result<Native> {
        let (ready_tx, ready_rx) =
            std::sync::mpsc::channel::<Result<(String, Option<String>, u32, Arc<Mutex<Engine>>)>>();
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let rec = Arc::new(Recording {
            active: AtomicBool::new(false),
            buf: Mutex::new(vec![]),
            channels: Mutex::new(1),
            sample_rate: Mutex::new(48000),
        });
        let rec2 = rec.clone();
        // cpal streams are not Send on every platform: own them on a thread.
        std::thread::spawn(move || {
            let res = (|| -> Result<(cpal::Stream, Option<cpal::Stream>, String, Option<String>, u32, Arc<Mutex<Engine>>)> {
                let host = cpal::default_host();
                let device = host.default_output_device().ok_or_else(|| anyhow!("no audio output device"))?;
                let name = device.name().unwrap_or_else(|_| "output".into());
                let supported = device.default_output_config()?;
                let sr = supported.sample_rate().0;
                let channels = supported.channels() as usize;
                let mut engine = Engine::new(sr as f32);
                crate::server::install_plugin_host(&mut engine);
                engine.set_project(project);
                let engine = Arc::new(Mutex::new(engine));
                let e2 = engine.clone();
                let mut l = vec![0f32; 8192];
                let mut r = vec![0f32; 8192];
                let config: cpal::StreamConfig = supported.clone().into();
                let err = |e| eprintln!("audio stream error: {e}");
                let mut render = move |frames: usize| {
                    let n = frames.min(l.len());
                    e2.lock().process(&mut l[..n], &mut r[..n]);
                    (l[..n].to_vec(), r[..n].to_vec())
                };
                let stream = match supported.sample_format() {
                    cpal::SampleFormat::F32 => device.build_output_stream(
                        &config,
                        move |data: &mut [f32], _| {
                            let frames = data.len() / channels;
                            let (l, r) = render(frames);
                            for (i, f) in data.chunks_mut(channels).enumerate() {
                                for (c, s) in f.iter_mut().enumerate() {
                                    *s = if c % 2 == 0 { l.get(i).copied().unwrap_or(0.0) } else { r.get(i).copied().unwrap_or(0.0) };
                                }
                            }
                        },
                        err,
                        None,
                    )?,
                    cpal::SampleFormat::I16 => device.build_output_stream(
                        &config,
                        move |data: &mut [i16], _| {
                            let frames = data.len() / channels;
                            let (l, r) = render(frames);
                            for (i, f) in data.chunks_mut(channels).enumerate() {
                                for (c, s) in f.iter_mut().enumerate() {
                                    let v = if c % 2 == 0 { l.get(i).copied().unwrap_or(0.0) } else { r.get(i).copied().unwrap_or(0.0) };
                                    *s = (v.clamp(-1.0, 1.0) * 32767.0) as i16;
                                }
                            }
                        },
                        err,
                        None,
                    )?,
                    f => return Err(anyhow!("unsupported sample format {f:?}")),
                };
                stream.play()?;

                // Recording input (optional).
                let mut input_name = None;
                let input_stream = host.default_input_device().and_then(|dev| {
                    let cfg = dev.default_input_config().ok()?;
                    if cfg.sample_format() != cpal::SampleFormat::F32 {
                        return None;
                    }
                    *rec2.channels.lock() = cfg.channels();
                    *rec2.sample_rate.lock() = cfg.sample_rate().0;
                    input_name = dev.name().ok();
                    let rec3 = rec2.clone();
                    let s = dev
                        .build_input_stream(
                            &cfg.into(),
                            move |data: &[f32], _| {
                                if rec3.active.load(Ordering::Relaxed) {
                                    rec3.buf.lock().extend_from_slice(data);
                                }
                            },
                            |e| eprintln!("audio input error: {e}"),
                            None,
                        )
                        .ok()?;
                    s.play().ok()?;
                    Some(s)
                });
                Ok((stream, input_stream, name, input_name, sr, engine))
            })();
            match res {
                Ok((stream, input, name, input_name, sr, engine)) => {
                    let _ = ready_tx.send(Ok((name, input_name, sr, engine)));
                    let _ = stop_rx.recv();
                    drop(input);
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        });
        let (device, input, sample_rate, engine) = ready_rx
            .recv()
            .map_err(|_| anyhow!("audio thread failed"))??;
        let n = Native {
            engine,
            stop: stop_tx,
            device,
            input,
            sample_rate,
            rec,
            rec_start: Mutex::new(None),
        };
        n.load_missing_samples(folder);
        Ok(n)
    }

    pub fn set_project(&self, project: Project, folder: &Folder) {
        self.engine.lock().set_project(project);
        self.load_missing_samples(folder);
    }

    fn load_missing_samples(&self, folder: &Folder) {
        let missing: Vec<String> = {
            let e = self.engine.lock();
            e.required_samples()
                .into_iter()
                .filter(|p| !e.has_sample(p))
                .collect()
        };
        for path in missing {
            if let Some(file) = folder.resolve(&path) {
                match crate::decode::decode_file(folder.fs.as_ref(), &file) {
                    Ok(data) => self.engine.lock().set_sample(&path, data),
                    Err(e) => eprintln!("sample {path}: {e}"),
                }
            }
        }
    }

    pub fn status_json(&self) -> Value {
        json!({
            "available": true,
            "enabled": true,
            "device": self.device,
            "input": self.input,
            "sampleRate": self.sample_rate,
            "errors": self.engine.lock().device_errors,
        })
    }

    pub fn meters_json(&self) -> Value {
        let mut e = self.engine.lock();
        let m = e.take_meters();
        json!({
            "t": "native.meters",
            "position": e.position(),
            "playing": e.is_playing(),
            "loopLength": e.loop_length(),
            "recording": self.rec.active.load(Ordering::Relaxed),
            "inserts": m.inserts,
            "channels": m.channels,
        })
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.stop.send(());
    }
}

/// Handle `native.*` messages from the UI.
pub async fn handle(app: Arc<App>, t: &str, v: &Value) -> Option<Value> {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    match t {
        "native.enable" => {
            let project = app.project();
            let folder = app.folder();
            let res = tokio::task::spawn_blocking(move || Native::start(project, &folder)).await;
            let reply = match res {
                Ok(Ok(n)) => {
                    let st = n.status_json();
                    *app.native().lock() = Some(n);
                    st
                }
                Ok(Err(e)) => json!({"available": true, "enabled": false, "error": e.to_string()}),
                Err(e) => json!({"available": true, "enabled": false, "error": e.to_string()}),
            };
            app.send_all(json!({"t": "native", "status": reply}));
            None
        }
        "native.disable" => {
            *app.native().lock() = None;
            app.send_all(json!({"t": "native", "status": {"available": true, "enabled": false}}));
            None
        }
        _ => {
            let guard = app.native().lock();
            let n = guard.as_ref()?;
            let mut e = n.engine.lock();
            match t {
                "native.play" => {
                    let pattern = s("pattern");
                    e.set_mode(if pattern.is_empty() {
                        PlayMode::Song
                    } else {
                        PlayMode::Pattern(pattern)
                    });
                    e.play();
                }
                "native.pause" => e.pause(),
                "native.stop" => {
                    e.stop();
                    drop(e);
                    drop(guard);
                    finish_recording(&app);
                }
                "native.seek" => e.seek(v.get("beat").and_then(|b| b.as_f64()).unwrap_or(0.0)),
                "native.note" => {
                    let key = v.get("key").and_then(|k| k.as_u64()).unwrap_or(60).min(127) as u8;
                    if v.get("on").and_then(|o| o.as_bool()).unwrap_or(false) {
                        e.note_on(
                            &s("channel"),
                            key,
                            v.get("velocity").and_then(|x| x.as_f64()).unwrap_or(0.8) as f32,
                        );
                    } else {
                        e.note_off(&s("channel"), key);
                    }
                }
                "native.record" => {
                    let track = v.get("track").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                    n.rec.buf.lock().clear();
                    *n.rec_start.lock() = Some((e.position(), track));
                    n.rec.active.store(true, Ordering::Relaxed);
                    e.set_mode(PlayMode::Song);
                    e.play();
                }
                _ => {}
            }
            None
        }
    }
}

/// Write the recorded take to `samples/` and place it on the playlist.
fn finish_recording(app: &Arc<App>) {
    let (data, channels, sr, start) = {
        let guard = app.native().lock();
        let Some(n) = guard.as_ref() else { return };
        if !n.rec.active.swap(false, Ordering::Relaxed) {
            return;
        }
        let data = std::mem::take(&mut *n.rec.buf.lock());
        let start = n.rec_start.lock().take();
        let channels = *n.rec.channels.lock() as usize;
        let sr = *n.rec.sample_rate.lock();
        (data, channels, sr, start)
    };
    let Some((start_beat, track)) = start else {
        return;
    };
    if data.is_empty() {
        return;
    }
    let frames = data.len() / channels.max(1);
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);
    for f in data.chunks(channels.max(1)) {
        left.push(f[0]);
        right.push(*f.get(1).unwrap_or(&f[0]));
    }
    let audio = rosaclef_engine::render::Audio {
        sample_rate: sr as f32,
        left,
        right,
    };
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let rel =
        rosaclef_studio::library::unique_sample_path(&app.folder(), &format!("take-{stamp}.wav"));
    if folder::write_atomic(
        &app.folder().dir.join(&rel),
        &rosaclef_engine::render::encode_wav(&audio, 24),
    )
    .is_err()
    {
        return;
    }
    let mut project = app.project();
    let beats = audio.duration() / project.seconds_per_beat();
    let track = TrackIx(track.min(project.playlist.tracks.len().saturating_sub(1) as u32));
    project.playlist.clips.push(Clip {
        pattern: String::new(),
        sample: rel.clone(),
        track,
        start: start_beat,
        length: beats.max(0.25),
        offset: 0.0,
        gain: 1.0,
        mixer: InsertIx::MASTER,
    });
    app.apply_edit(project);
    app.send_all(json!({"t": "recorded", "path": rel}));
}
