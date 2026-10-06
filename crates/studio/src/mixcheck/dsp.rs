//! The measuring instruments: a real FFT, the BS.1770 K-weighting filter,
//! a 4× oversampling true-peak detector, and the band layouts (24 critical
//! bands for the masking model, six broad bands for the spectrum).

use std::f64::consts::PI;

// ---------------------------------------------------------------------- FFT

/// A real FFT of a fixed power-of-two size (a complex FFT of half the size
/// with the usual split), returning the one-sided power spectrum.
pub struct RealFft {
    n: usize,
    /// Bit-reversal permutation and twiddles of the half-size complex FFT.
    rev: Vec<usize>,
    tw: Vec<(f32, f32)>,
    /// Twiddles of the split step.
    split: Vec<(f32, f32)>,
    re: Vec<f32>,
    im: Vec<f32>,
}

impl RealFft {
    pub fn new(n: usize) -> RealFft {
        assert!(n.is_power_of_two() && n >= 8);
        let h = n / 2;
        let bits = h.trailing_zeros();
        let rev = (0..h)
            .map(|i| i.reverse_bits() >> (usize::BITS - bits))
            .collect();
        let tw = (0..h / 2)
            .map(|k| {
                let a = -2.0 * PI * k as f64 / h as f64;
                (a.cos() as f32, a.sin() as f32)
            })
            .collect();
        let split = (0..h)
            .map(|k| {
                let a = -2.0 * PI * k as f64 / n as f64;
                (a.cos() as f32, a.sin() as f32)
            })
            .collect();
        RealFft {
            n,
            rev,
            tw,
            split,
            re: vec![0.0; h],
            im: vec![0.0; h],
        }
    }

    /// `|X[k]|²` for k = 0 ..= n/2 into `out` (n/2 + 1 values).
    pub fn power(&mut self, x: &[f32], out: &mut [f32]) {
        let h = self.n / 2;
        for i in 0..h {
            let j = self.rev[i];
            self.re[j] = x[2 * i];
            self.im[j] = x[2 * i + 1];
        }
        let (re, im) = (&mut self.re, &mut self.im);
        let mut size = 2;
        while size <= h {
            let half = size / 2;
            let step = h / size;
            for start in (0..h).step_by(size) {
                for k in 0..half {
                    let (wr, wi) = self.tw[k * step];
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            size *= 2;
        }
        // Split the packed spectrum into the real input's.
        out[0] = (re[0] + im[0]).powi(2);
        out[h] = (re[0] - im[0]).powi(2);
        for k in 1..h {
            let (zr, zi) = (re[k], im[k]);
            let (cr, ci) = (re[h - k], -im[h - k]);
            let (er, ei) = (0.5 * (zr + cr), 0.5 * (zi + ci));
            let (or_, oi) = (0.5 * (zi - ci), -0.5 * (zr - cr));
            let (wr, wi) = self.split[k];
            let xr = er + or_ * wr - oi * wi;
            let xi = ei + or_ * wi + oi * wr;
            out[k] = xr * xr + xi * xi;
        }
    }
}

/// A Tukey window of `n` points (cosine tapers over a share `taper` of it,
/// flat between), and the sum of its squares: hops measured back to back
/// count almost every sample fully.
pub fn tukey(n: usize, taper: f64) -> (Vec<f32>, f64) {
    let edge = (taper * n as f64 / 2.0).max(1.0);
    let w: Vec<f32> = (0..n)
        .map(|i| {
            let d = (i as f64 + 0.5).min(n as f64 - i as f64 - 0.5);
            if d >= edge {
                1.0
            } else {
                (0.5 - 0.5 * (PI * d / edge).cos()) as f32
            }
        })
        .collect();
    let s = w.iter().map(|x| (*x as f64).powi(2)).sum();
    (w, s)
}

/// A Hann window of `n` points, and the sum of its squares.
pub fn hann(n: usize) -> (Vec<f32>, f64) {
    let w: Vec<f32> = (0..n)
        .map(|i| (0.5 - 0.5 * (2.0 * PI * i as f64 / n as f64).cos()) as f32)
        .collect();
    let s = w.iter().map(|x| (*x as f64).powi(2)).sum();
    (w, s)
}

// ------------------------------------------------------------------- bands

/// Edges (Hz) of the 24 critical bands (Zwicker's Bark scale). The last
/// band runs on to the Nyquist frequency.
pub const BARK_EDGES: [f32; 25] = [
    0.0, 100.0, 200.0, 300.0, 400.0, 510.0, 630.0, 770.0, 920.0, 1080.0, 1270.0, 1480.0, 1720.0,
    2000.0, 2320.0, 2700.0, 3150.0, 3700.0, 4400.0, 5300.0, 6400.0, 7700.0, 9500.0, 12000.0,
    15500.0,
];
pub const BARKS: usize = 24;

/// The spectrum's six broad bands.
pub const BAND_NAMES: [&str; 6] = [
    "sub<60",
    "bass60-250",
    "lowmid250-500",
    "mid500-2k",
    "himid2-6k",
    "air6k+",
];
pub const BAND_EDGES: [f32; 7] = [0.0, 60.0, 250.0, 500.0, 2000.0, 6000.0, f32::INFINITY];
pub const BANDS: usize = 6;

/// Centre (geometric) frequency of a critical band.
pub fn bark_centre(b: usize) -> f32 {
    let lo = BARK_EDGES[b].max(50.0);
    let hi = if b + 1 == BARKS {
        16000.0
    } else {
        BARK_EDGES[b + 1]
    };
    (lo * hi).sqrt()
}

/// The band (in `edges`) of each FFT bin; `usize::MAX` for the DC bin.
pub fn bin_bands(n: usize, sr: f32, edges: &[f32]) -> Vec<usize> {
    let bands = edges.len() - 1;
    (0..=n / 2)
        .map(|k| {
            if k == 0 {
                return usize::MAX;
            }
            let f = k as f32 * sr / n as f32;
            (0..bands)
                .rev()
                .find(|b| f >= edges[*b])
                .unwrap_or(0)
                .min(bands - 1)
        })
        .collect()
}

// ------------------------------------------------------------ K-weighting

#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

/// The K-weighting of ITU-R BS.1770 (a high shelf for the head, then the
/// RLB high-pass), designed for any sample rate: at 48 kHz it is the
/// standard's own filter.
#[derive(Clone, Copy, Debug)]
pub struct KWeight {
    shelf: Biquad,
    hp: Biquad,
}

impl KWeight {
    pub fn new(sr: f32) -> KWeight {
        let fs = sr as f64;
        // Stage 1: the shelf.
        let (g, f0, q) = (3.999843853973347, 1681.974450955533, 0.7071752369554196);
        let k = (PI * f0 / fs).tan();
        let vh = 10f64.powf(g / 20.0);
        let vb = vh.powf(0.4996667741545416);
        let a0 = 1.0 + k / q + k * k;
        let shelf = Biquad {
            b: [
                (vh + vb * k / q + k * k) / a0,
                2.0 * (k * k - vh) / a0,
                (vh - vb * k / q + k * k) / a0,
            ],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
            z: [0.0; 2],
        };
        // Stage 2: the high-pass.
        let (f0, q) = (38.13547087602444, 0.5003270373238773);
        let k = (PI * f0 / fs).tan();
        let a0 = 1.0 + k / q + k * k;
        let hp = Biquad {
            b: [1.0, -2.0, 1.0],
            a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
            z: [0.0; 2],
        };
        KWeight { shelf, hp }
    }

    #[inline]
    pub fn run(&mut self, x: f32) -> f64 {
        self.hp.run(self.shelf.run(x as f64))
    }
}

/// Loudness (LUFS) of a K-weighted mean square summed over the channels.
pub fn lufs(kms: f64) -> f64 {
    -0.691 + 10.0 * kms.max(1e-20).log10()
}

// --------------------------------------------------------------- true peak

const TP_PHASES: usize = 4;
const TP_TAPS: usize = 12;

/// Inter-sample peaks: the signal oversampled 4× by a windowed-sinc
/// interpolator (48 taps), as ITU-R BS.1770 Annex 2 describes.
#[derive(Clone)]
pub struct TruePeak {
    h: [[f32; TP_TAPS]; TP_PHASES],
    hist: [[f32; TP_TAPS]; 2],
    at: usize,
}

impl Default for TruePeak {
    fn default() -> Self {
        Self::new()
    }
}

impl TruePeak {
    pub fn new() -> TruePeak {
        let n = TP_PHASES * TP_TAPS;
        let mid = (n as f64 - 1.0) / 2.0;
        let mut h = [[0f32; TP_TAPS]; TP_PHASES];
        for (i, slot) in (0..n).map(|i| (i, i)) {
            let t = (slot as f64 - mid) / TP_PHASES as f64;
            let sinc = if t.abs() < 1e-12 {
                1.0
            } else {
                (PI * t).sin() / (PI * t)
            };
            // Blackman–Harris window.
            let x = 2.0 * PI * (i as f64 + 0.5) / n as f64;
            let w =
                0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos();
            h[i % TP_PHASES][i / TP_PHASES] = (sinc * w) as f32;
        }
        // Each phase passes DC at unity.
        for ph in &mut h {
            let s: f32 = ph.iter().sum();
            ph.iter_mut().for_each(|x| *x /= s);
        }
        TruePeak {
            h,
            hist: [[0.0; TP_TAPS]; 2],
            at: 0,
        }
    }

    /// Feed one stereo frame; returns the largest absolute value among the
    /// oversampled points it produces (and the sample itself).
    #[inline]
    pub fn push(&mut self, l: f32, r: f32) -> f32 {
        self.hist[0][self.at] = l;
        self.hist[1][self.at] = r;
        self.at = (self.at + 1) % TP_TAPS;
        let mut m = l.abs().max(r.abs());
        for ch in &self.hist {
            for ph in &self.h {
                let mut acc = 0.0;
                for (j, c) in ph.iter().enumerate() {
                    acc += c * ch[(self.at + TP_TAPS - 1 - j) % TP_TAPS];
                }
                m = m.max(acc.abs());
            }
        }
        m
    }
}

// --------------------------------------------------------------- helpers

pub fn db(power: f64) -> f64 {
    10.0 * power.max(1e-20).log10()
}

pub fn amp_db(a: f64) -> f64 {
    20.0 * a.max(1e-10).log10()
}

/// Rounded to 0.1 (the report's resolution).
pub fn r1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_finds_a_sine() {
        let n = 1024;
        let mut fft = RealFft::new(n);
        let x: Vec<f32> = (0..n)
            .map(|i| (2.0 * PI * 37.0 * i as f64 / n as f64).sin() as f32)
            .collect();
        let mut p = vec![0.0; n / 2 + 1];
        fft.power(&x, &mut p);
        let peak = (0..p.len()).max_by(|a, b| p[*a].total_cmp(&p[*b])).unwrap();
        assert_eq!(peak, 37);
        // Parseval: Σ|X|² = N Σx² (one-sided: interior bins twice).
        let total: f64 = p[0] as f64
            + p[n / 2] as f64
            + 2.0 * p[1..n / 2].iter().map(|v| *v as f64).sum::<f64>();
        let energy: f64 = x.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() * n as f64;
        assert!((total / energy - 1.0).abs() < 1e-3, "{total} {energy}");
    }

    #[test]
    fn k_weighting_is_the_standards_at_48k() {
        // A 1 kHz sine at -20 dBFS (peak) reads about -23.0 LUFS on one channel... in stereo -20 dB
        // RMS per channel: the standard's reference is -3.01 + 0.69 ≈ ... check the 997 Hz tone.
        let sr = 48000.0;
        let mut k = [KWeight::new(sr), KWeight::new(sr)];
        let amp = 10f64.powf(-20.0 / 20.0);
        let n = 48000 * 2;
        let mut sum = 0.0;
        for i in 0..n {
            let x = (amp * (2.0 * PI * 997.0 * i as f64 / sr as f64).sin()) as f32;
            let (a, b) = (k[0].run(x), k[1].run(x));
            if i >= 48000 {
                sum += a * a + b * b;
            }
        }
        // Stereo 997 Hz at -20 dBFS peak: -20 - 3.01 + 3.01 (two channels) ≈ -20 LUFS... within 0.2.
        let l = lufs(sum / 48000.0);
        assert!((l - -20.0).abs() < 0.2, "{l}");
    }

    #[test]
    fn true_peak_sees_between_samples() {
        // A sine at fs/4 with its peaks between the samples: the samples read
        // 0.707 of the true peak (-3 dB).
        let mut tp = TruePeak::new();
        let mut m: f32 = 0.0;
        let mut s: f32 = 0.0;
        for i in 0..400 {
            let x = (PI / 2.0 * i as f64 + PI / 4.0).sin() as f32;
            s = s.max(x.abs());
            m = m.max(tp.push(x, x));
        }
        assert!(s < 0.72);
        assert!(m > 0.95 && m < 1.05, "{m}");
    }
}
