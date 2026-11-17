//! The `listen` image: per sound, a log-frequency spectrogram, the waveform
//! (peak and RMS envelopes) and the momentary loudness, with labelled axes.
//! Several sounds tile as labelled columns, kept under ~1.15 MP like
//! modeler's views.

use sfx_graph::{Audio, gain_to_db, loudness};

use crate::raster::{Canvas, colormap, colormap_diverging, series};
use crate::stft::{self, db};

/// Pixel budget of one image.
pub const MAX_PIXELS: usize = 1_150_000;

const BG: [u8; 4] = [18, 18, 22, 255];
const GRID: [u8; 4] = [70, 70, 80, 255];
const AXIS_W: usize = 44;
const TITLE_H: usize = 22;
const SPEC_H: usize = 300;
const WAVE_H: usize = 110;
const LOUD_H: usize = 80;
const GAP: usize = 14;
const PANEL_H: usize = TITLE_H + SPEC_H + GAP + WAVE_H + GAP + LOUD_H + GAP;

/// What part of the sound to show.
#[derive(Debug, Clone, Copy, Default)]
pub struct View {
    /// Seconds [a, b] (default: all).
    pub t: Option<[f32; 2]>,
    /// Hz [lo, hi] (default 20 .. 20k, or Nyquist).
    pub hz: Option<[f32; 2]>,
}

/// One column: a label and a sound.
pub struct Column<'a> {
    pub label: String,
    pub audio: &'a Audio,
}

/// The difference column: the spectrogram shows `a` minus `b` in dB
/// (diverging colours: blue = `a` quieter, red = louder), the waveform and
/// loudness curve show the sample difference `a - b`.
pub struct DiffColumn<'a> {
    pub label: String,
    pub a: &'a Audio,
    pub b: &'a Audio,
}

/// Largest dB difference the diverging map saturates at.
pub const DIFF_RANGE_DB: f32 = 24.0;

/// `a - b` sample by sample (the shorter padded with silence; a mono side
/// is repeated on both channels of a stereo one).
pub fn residual(a: &Audio, b: &Audio) -> Audio {
    let n = a.len().max(b.len());
    let k = a.channels.len().max(b.channels.len());
    let at = |x: &Audio, c: usize, i: usize| {
        x.channels
            .get(c.min(x.channels.len().saturating_sub(1)))
            .and_then(|ch| ch.get(i))
            .copied()
            .unwrap_or(0.0)
    };
    Audio {
        sample_rate: a.sample_rate,
        channels: (0..k)
            .map(|c| (0..n).map(|i| at(a, c, i) - at(b, c, i)).collect())
            .collect(),
    }
}

/// Width of each column so `cols` columns fit the budget.
fn column_width(cols: usize) -> usize {
    let cols = cols.max(1);
    (MAX_PIXELS / (PANEL_H * cols)).clamp(240, 1100)
}

/// Renders columns side by side and returns the PNG.
pub fn listen_png(cols: &[Column], view: View) -> Result<Vec<u8>, String> {
    listen_png_diff(cols, None, view)
}

/// [`listen_png`] with an optional last column showing a dB difference.
pub fn listen_png_diff(
    cols: &[Column],
    diff: Option<&DiffColumn>,
    view: View,
) -> Result<Vec<u8>, String> {
    let total = cols.len() + usize::from(diff.is_some());
    if total == 0 {
        return Err("nothing to show".into());
    }
    let w = column_width(total);
    let mut c = Canvas::new(w * total, PANEL_H, BG);
    let res = diff.map(|d| residual(d.a, d.b));
    for k in 0..total {
        let p = match (cols.get(k), diff, &res) {
            (Some(col), _, _) => panel(&col.label, col.audio, None, w, view),
            (None, Some(d), Some(r)) => panel(&d.label, r, Some((d.a, d.b)), w, view),
            _ => break,
        };
        c.blit(&p, k * w, 0);
        if k > 0 {
            c.line(
                ((k * w) as f64, 0.0),
                ((k * w) as f64, PANEL_H as f64),
                GRID,
                1,
            );
        }
    }
    c.to_png()
}

/// Power per pixel (x-major, SPEC_H rows) of `seg`'s spectrogram.
fn spec_grid(
    seg: &[f32],
    sr: u32,
    win: usize,
    hop: usize,
    span: f32,
    pw: usize,
    y_hz: &dyn Fn(usize) -> f32,
) -> Vec<f32> {
    let spec = stft::stft(seg, sr, win, hop);
    let mut out = vec![0.0f32; pw * SPEC_H];
    for x in 0..pw {
        let a0 = (x as f32 * span / hop as f32) as usize;
        let a1 = (((x + 1) as f32 * span / hop as f32) as usize)
            .max(a0 + 1)
            .min(spec.frames.len());
        if a0 >= spec.frames.len() {
            break;
        }
        let avg = spec.mean(a0..a1);
        for y in 0..SPEC_H {
            let (hi, lo) = (y_hz(y), y_hz(y + 1));
            let (b0, b1) = (spec.bin(lo), spec.bin(hi));
            out[x * SPEC_H + y] = if b1 > b0 + 1 {
                avg[b0..b1].iter().copied().fold(0.0, f32::max)
            } else {
                // Interpolate between bins at low frequencies.
                let fb = 0.5 * (lo + hi) * spec.win as f32 / sr as f32;
                let k = (fb.floor() as usize).min(avg.len() - 2);
                let f = fb - k as f32;
                avg[k] + (avg[k + 1] - avg[k]) * f
            };
        }
    }
    out
}

/// The colour scale of a difference spectrogram, top right of its panel.
fn legend(c: &mut Canvas, right: usize, top: usize) {
    let (bw, bh) = (120usize, 8usize);
    let x0 = right.saturating_sub(bw + 4);
    for i in 0..bw {
        let t = i as f32 / (bw - 1) as f32 * 2.0 - 1.0;
        c.rect(x0 + i, top + 20, 1, bh, colormap_diverging(t));
    }
    let lo = format!("-{DIFF_RANGE_DB:.0}");
    let hi = format!("+{DIFF_RANGE_DB:.0} dB");
    c.text(x0, top + 30, &lo, 1);
    c.text(x0 + bw / 2 - 3, top + 30, "0", 1);
    c.text(
        (x0 + bw).saturating_sub(Canvas::text_width(&hi, 1)),
        top + 30,
        &hi,
        1,
    );
    let key = "BLUE QUIETER  RED LOUDER";
    c.text(
        right.saturating_sub(Canvas::text_width(key, 1) + 4),
        top + 4,
        key,
        1,
    );
}

fn nice_step(span: f32, ticks: f32) -> f32 {
    let raw = span / ticks;
    let p = 10f32.powf(raw.log10().floor());
    for m in [1.0, 2.0, 5.0, 10.0] {
        if raw <= m * p {
            return m * p;
        }
    }
    10.0 * p
}

fn fmt_t(t: f32, step: f32) -> String {
    if step >= 1.0 {
        format!("{t:.0}s")
    } else if step >= 0.1 {
        format!("{t:.1}s")
    } else if step >= 0.01 {
        format!("{t:.2}s")
    } else {
        format!("{t:.3}s")
    }
}

fn panel(
    label: &str,
    a: &Audio,
    spec_diff: Option<(&Audio, &Audio)>,
    w: usize,
    view: View,
) -> Canvas {
    let sr = a.sample_rate;
    let mut c = Canvas::new(w, PANEL_H, BG);
    let mono = a.mix_mono();
    let total = a.seconds().max(1.0 / sr as f32);
    let [t0, t1] = view.t.unwrap_or([0.0, total]);
    let (t0, t1) = (t0.max(0.0), t1.max(t0 + 1e-3));
    let nyq = sr as f32 / 2.0;
    let [f0, f1] = view.hz.unwrap_or([20.0, 20_000.0f32.min(nyq)]);
    let (f0, f1) = (f0.max(1.0), f1.min(nyq).max(f0 * 1.01));
    let pw = w.saturating_sub(AXIS_W + 8).max(16);
    let x_of = |t: f32| AXIS_W as f64 + (t - t0) as f64 / (t1 - t0) as f64 * pw as f64;

    // Title.
    let lufs = loudness::integrated(&a.channels, sr);
    let title = format!(
        "{}  {:.2}s  pk {:.1}  {:.1} LUFS",
        label,
        total,
        gain_to_db(a.peak()),
        lufs
    );
    c.text(4, 3, &title, 2);

    // Spectrogram.
    let top = TITLE_H;
    let i0 = ((t0 * sr as f32) as usize).min(mono.len());
    let i1 = ((t1 * sr as f32).ceil() as usize).min(mono.len()).max(i0);
    let seg = &mono[i0..i1];
    let win = crate::descriptors::window_for(seg.len(), sr);
    let span = (seg.len() as f32 / pw as f32).max(1.0);
    let hop = ((span as usize).min(win / 4)).max(1);
    let y_hz = |y: usize| f0 * (f1 / f0).powf(1.0 - y as f32 / SPEC_H as f32);
    let grid = spec_grid(seg, sr, win, hop, span, pw, &y_hz);
    let other = spec_diff.map(|(x, y)| {
        let part = |au: &Audio| {
            let m = au.mix_mono();
            let (s0, s1) = (i0.min(m.len()), i1.min(m.len()));
            let mut v = m[s0..s1].to_vec();
            v.resize(i1 - i0, 0.0);
            spec_grid(&v, sr, win, hop, span, pw, &y_hz)
        };
        (part(x), part(y))
    });
    for x in 0..pw {
        for y in 0..SPEC_H {
            let colour = match &other {
                None => colormap((db(grid[x * SPEC_H + y]) + 90.0) / 90.0),
                Some((pa, pb)) => {
                    let (da, db_) = (db(pa[x * SPEC_H + y]), db(pb[x * SPEC_H + y]));
                    if da < -75.0 && db_ < -75.0 {
                        colormap_diverging(0.0)
                    } else {
                        colormap_diverging((da.max(-90.0) - db_.max(-90.0)) / DIFF_RANGE_DB)
                    }
                }
            };
            c.set((AXIS_W + x) as i64, (top + y) as i64, colour);
        }
    }
    if other.is_some() {
        legend(&mut c, AXIS_W + pw, top);
    }
    // Octave grid (true octaves of 1 kHz) with nominal labels.
    const OCTAVES: [(f32, &str); 10] = [
        (31.25, "31"),
        (62.5, "63"),
        (125.0, "125"),
        (250.0, "250"),
        (500.0, "500"),
        (1000.0, "1k"),
        (2000.0, "2k"),
        (4000.0, "4k"),
        (8000.0, "8k"),
        (16000.0, "16k"),
    ];
    for (f, label) in OCTAVES {
        if f >= f0 && f <= f1 {
            let y = top as f64 + (1.0 - ((f / f0).ln() / (f1 / f0).ln()) as f64) * SPEC_H as f64;
            for x in (AXIS_W..AXIS_W + pw).step_by(3) {
                c.set(x as i64, y as i64, GRID);
            }
            c.text(2, (y as usize).saturating_sub(4), label, 1);
        }
    }
    // Time grid (all rows).
    let step = nice_step(t1 - t0, 6.0);
    let mut t = (t0 / step).ceil() * step;
    while t <= t1 + 1e-6 {
        let x = x_of(t);
        for y in (top..PANEL_H - GAP).step_by(3) {
            c.set(x as i64, y as i64, GRID);
        }
        let label = fmt_t(t, step);
        let lx = (x as usize)
            .saturating_sub(10)
            .min(w.saturating_sub(Canvas::text_width(&label, 1)));
        c.text(lx, PANEL_H - GAP + 2, &label, 1);
        t += step;
    }

    // Waveform: peak (dim) and RMS (bright) envelopes per column.
    let wt = top + SPEC_H + GAP;
    let mid = wt as f64 + WAVE_H as f64 / 2.0;
    let half = WAVE_H as f64 / 2.0 - 2.0;
    c.line((AXIS_W as f64, mid), ((AXIS_W + pw) as f64, mid), GRID, 1);
    c.text(2, wt, "wave", 1);
    let col_pk = series(0);
    let dim = [col_pk[0] / 2, col_pk[1] / 2, col_pk[2] / 2, 255];
    for x in 0..pw {
        let s0 = i0 + (x as f32 * span) as usize;
        let s1 = (i0 + ((x + 1) as f32 * span) as usize)
            .min(i1)
            .max(s0 + 1)
            .min(mono.len());
        if s0 >= mono.len() {
            break;
        }
        let chunk = &mono[s0..s1];
        let (mn, mx) = chunk
            .iter()
            .fold((0.0f32, 0.0f32), |m, v| (m.0.min(*v), m.1.max(*v)));
        let r = (chunk.iter().map(|v| v * v).sum::<f32>() / chunk.len() as f32).sqrt();
        let px = (AXIS_W + x) as f64;
        c.line(
            (px, mid - f64::from(mx.clamp(-1.0, 1.0)) * half),
            (px, mid - f64::from(mn.clamp(-1.0, 1.0)) * half),
            dim,
            1,
        );
        c.line(
            (px, mid - f64::from(r.min(1.0)) * half),
            (px, mid + f64::from(r.min(1.0)) * half),
            col_pk,
            1,
        );
    }

    // Momentary loudness, -60 .. 0 LUFS.
    let lt = wt + WAVE_H + GAP;
    let y_l = |l: f32| lt as f64 + f64::from((-l).clamp(0.0, 60.0) / 60.0) * LOUD_H as f64;
    for l in [-12.0, -24.0, -36.0, -48.0] {
        let y = y_l(l);
        for x in (AXIS_W..AXIS_W + pw).step_by(3) {
            c.set(x as i64, y as i64, GRID);
        }
        c.text(2, (y as usize).saturating_sub(4), &format!("{l:.0}"), 1);
    }
    // A 400 ms window smears a short sound into one blob: shrink it to a
    // quarter of the sound (at least 20 ms).
    let window = (f64::from(total) / 4.0).clamp(0.02, 0.4);
    c.text(2, lt, &format!("LUFS {:.0}ms", window * 1000.0), 1);
    let hop_s = (f64::from(t1 - t0) / pw as f64).max(0.001);
    let m = loudness::curve(&a.channels, sr, window, hop_s);
    let mut prev: Option<(f64, f64)> = None;
    for (k, l) in m.iter().enumerate() {
        let t = (k as f64 * hop_s) as f32;
        if t < t0 || t > t1 {
            continue;
        }
        let p = (x_of(t), y_l(*l));
        if let Some(q) = prev {
            c.line(q, p, series(1), 1);
        }
        prev = Some(p);
    }
    c
}
