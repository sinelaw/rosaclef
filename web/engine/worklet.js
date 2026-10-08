// Rosaclef AudioWorklet: runs the Rust engine (compiled to WebAssembly)
// inside the browser's audio thread.
//
// This is the one file in the frontend that inty does not check: an
// AudioWorklet processor must `extend AudioWorkletProcessor`, and inty
// deliberately leaves class inheritance out of its type system. It is kept
// deliberately small; all application logic lives in the checked modules.

// TextEncoder/TextDecoder are not available in AudioWorkletGlobalScope.
function utf8Encode(str) {
  const out = [];
  for (const ch of str) {
    let c = ch.codePointAt(0);
    if (c < 0x80) out.push(c);
    else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
    else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
  }
  return new Uint8Array(out);
}

function utf8Decode(bytes) {
  let s = "";
  for (let i = 0; i < bytes.length; ) {
    const b = bytes[i];
    let c;
    if (b < 0x80) {
      c = b;
      i += 1;
    } else if (b < 0xe0) {
      c = ((b & 31) << 6) | (bytes[i + 1] & 63);
      i += 2;
    } else if (b < 0xf0) {
      c = ((b & 15) << 12) | ((bytes[i + 1] & 63) << 6) | (bytes[i + 2] & 63);
      i += 3;
    } else {
      c = ((b & 7) << 18) | ((bytes[i + 1] & 63) << 12) | ((bytes[i + 2] & 63) << 6) | (bytes[i + 3] & 63);
      i += 4;
    }
    s += String.fromCodePoint(c);
  }
  return s;
}

class RosaclefProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.wasm = null;
    this.mem = null;
    this.frame = 0;
    this.recording = false;
    // The last transport command applied (numbered by the page), told with each status.
    this.transport = 0;
    // Messages that arrive before the WebAssembly module is ready wait for it.
    this.ready = new Promise((resolve) => {
      this.resolveReady = resolve;
    });
    this.port.onmessage = (e) => this.onMessage(e.data);
  }

  bytes(str) {
    const b = utf8Encode(str);
    const ptr = this.wasm.rc_alloc(b.length);
    new Uint8Array(this.wasm.memory.buffer, ptr, b.length).set(b);
    return [ptr, b.length];
  }

  withStr(str, fn) {
    const [ptr, len] = this.bytes(str);
    try {
      return fn(ptr, len);
    } finally {
      this.wasm.rc_free(ptr, len);
    }
  }

  result() {
    const ptr = this.wasm.rc_result_ptr();
    const len = this.wasm.rc_result_len();
    return utf8Decode(new Uint8Array(this.wasm.memory.buffer, ptr, len));
  }

  async onMessage(m) {
    if (m.t === "init") {
      const { instance } = await WebAssembly.instantiate(m.wasm, {});
      this.wasm = instance.exports;
      this.wasm.rc_init(sampleRate);
      this.resolveReady();
      this.port.postMessage({ t: "ready", sampleRate });
      return;
    }
    if (!this.wasm) await this.ready;
    const w = this.wasm;
    switch (m.t) {
      case "project": {
        const status = this.withStr(m.json, (p, l) => w.rc_set_project(p, l));
        const res = this.result();
        if (status === 0) {
          const r = JSON.parse(res || "{}");
          this.port.postMessage({ t: "loaded", missing: r.samples || [], presets: r.presets || [] });
        } else {
          this.port.postMessage({ t: "loadError", message: res });
        }
        break;
      }
      // A soundfont preset, decoded by the page's font worker, arrives in
      // small steps (each acknowledged) so no single message holds up the audio.
      case "presetBegin": {
        const h = new Uint8Array(m.header);
        const ptr = w.rc_alloc(h.length);
        new Uint8Array(w.memory.buffer, ptr, h.length).set(h);
        const ok = this.withStr(m.font, (p, l) => w.rc_preset_begin(p, l, m.bank, m.program, ptr, h.length));
        w.rc_free(ptr, h.length);
        this.port.postMessage({ t: "presetAck", ok: ok === 0 });
        break;
      }
      case "presetData": {
        const ptr = w.rc_preset_sample(m.index);
        if (ptr) new Int16Array(w.memory.buffer, ptr + m.offset * 2, m.data.length).set(m.data);
        this.port.postMessage({ t: "presetAck", ok: ptr !== 0 });
        break;
      }
      case "presetEnd":
        w.rc_preset_end();
        this.port.postMessage({ t: "presetAck", ok: true });
        break;
      case "sample": {
        const frames = m.channels[0].length;
        const n = m.channels.length;
        const bytes = frames * n * 4;
        const ptr = w.rc_alloc(bytes);
        const view = new Float32Array(w.memory.buffer, ptr, frames * n);
        m.channels.forEach((c, i) => view.set(c, i * frames));
        this.withStr(m.path, (p, l) => w.rc_set_sample(p, l, m.sampleRate, n, frames, ptr));
        w.rc_free(ptr, bytes);
        break;
      }
      case "play":
        this.transport = m.transport || this.transport;
        if (m.countIn > 0) w.rc_play_count_in(m.countIn);
        else w.rc_play();
        break;
      case "metronome":
        w.rc_set_metronome(m.on ? 1 : 0);
        break;
      case "openEnded":
        w.rc_set_open_ended(m.on ? 1 : 0);
        break;
      case "pause":
        this.transport = m.transport || this.transport;
        w.rc_pause();
        break;
      case "stop":
        this.transport = m.transport || this.transport;
        w.rc_stop();
        break;
      case "mode":
        this.withStr(m.pattern, (p, l) => w.rc_set_mode(p, l));
        break;
      case "seek":
        this.transport = m.transport || this.transport;
        w.rc_seek(m.beat);
        break;
      case "note":
        this.withStr(m.channel, (p, l) => w.rc_note(p, l, m.key, m.velocity, m.on ? 1 : 0));
        break;
      case "record":
        this.recording = m.on;
        break;
    }
  }

  process(inputs, outputs) {
    const out = outputs[0];
    const w = this.wasm;
    if (!w || !out || out.length === 0) return true;
    const n = out[0].length;
    w.rc_process(n);
    const l = new Float32Array(w.memory.buffer, w.rc_left(), n);
    const r = new Float32Array(w.memory.buffer, w.rc_right(), n);
    out[0].set(l);
    if (out.length > 1) out[1].set(r);

    if (this.recording) {
      const input = inputs[0];
      if (input && input.length > 0) {
        const a = input[0].slice();
        const b = (input[1] || input[0]).slice();
        this.port.postMessage({ t: "rec", left: a, right: b }, [a.buffer, b.buffer]);
      }
    }

    // ~40 status updates per second.
    this.frame += n;
    if (this.frame >= sampleRate / 40) {
      this.frame = 0;
      const count = w.rc_meters();
      const meters = new Float32Array(w.memory.buffer, w.rc_meters_ptr(), count).slice();
      this.port.postMessage({
        t: "status",
        position: w.rc_position(),
        playing: w.rc_is_playing() === 1,
        transport: this.transport,
        loopLength: w.rc_loop_length(),
        meters,
      });
    }
    return true;
  }
}

registerProcessor("rosaclef", RosaclefProcessor);
