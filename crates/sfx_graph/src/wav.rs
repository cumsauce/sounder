//! WAV files (hound): writing renders, reading `sample` and `convolve` inputs.

use std::path::Path;

use crate::Audio;

/// Sample format of a written WAV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bits {
    Int16,
    Int24,
    Float32,
}

/// Writes `audio` to `path`. Integer formats clamp to full scale (no
/// dither: the bytes stay a pure function of the samples).
pub fn write(path: &Path, audio: &Audio, bits: Bits) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: audio.channels.len().max(1) as u16,
        sample_rate: audio.sample_rate,
        bits_per_sample: match bits {
            Bits::Int16 => 16,
            Bits::Int24 => 24,
            Bits::Float32 => 32,
        },
        sample_format: if bits == Bits::Float32 {
            hound::SampleFormat::Float
        } else {
            hound::SampleFormat::Int
        },
    };
    let err = |e: hound::Error| format!("writing {}: {e}", path.display());
    let mut w = hound::WavWriter::create(path, spec).map_err(err)?;
    for i in 0..audio.len() {
        for c in &audio.channels {
            let x = c[i];
            match bits {
                Bits::Int16 => w
                    .write_sample((x.clamp(-1.0, 1.0) * 32767.0).round() as i16)
                    .map_err(err)?,
                Bits::Int24 => w
                    .write_sample((x.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32)
                    .map_err(err)?,
                Bits::Float32 => w.write_sample(x).map_err(err)?,
            }
        }
    }
    w.finalize().map_err(err)
}

/// The bytes a 16-bit WAV of `audio` would have (for determinism checks).
pub fn to_bytes16(audio: &Audio) -> Vec<u8> {
    let mut out = Vec::with_capacity(audio.len() * audio.channels.len() * 2);
    for i in 0..audio.len() {
        for c in &audio.channels {
            out.extend_from_slice(
                &((c[i].clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes(),
            );
        }
    }
    out
}

/// Reads a WAV of any integer or float format into -1..1 floats (one or
/// two channels; more are folded onto two).
pub fn read(path: &Path) -> Result<Audio, String> {
    let err = |e: hound::Error| format!("reading {}: {e}", path.display());
    let mut r = hound::WavReader::open(path).map_err(err)?;
    let spec = r.spec();
    let chans = usize::from(spec.channels.max(1));
    let flat: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().collect::<Result<_, _>>().map_err(err)?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.max(1) - 1)) as f32;
            r.samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .map_err(err)?
        }
    };
    let frames = flat.len() / chans;
    let out_ch = chans.min(2);
    let mut channels = vec![Vec::with_capacity(frames); out_ch];
    let k = (chans as f32 / out_ch as f32).ceil();
    for f in 0..frames {
        let mut acc = [0.0f32; 2];
        for c in 0..chans {
            acc[c % out_ch] += flat[f * chans + c];
        }
        for (c, ch) in channels.iter_mut().enumerate() {
            ch.push(if chans > 2 { acc[c] / k } else { acc[c] });
        }
    }
    Ok(Audio {
        sample_rate: spec.sample_rate,
        channels,
    })
}
