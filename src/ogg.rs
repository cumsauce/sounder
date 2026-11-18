//! Ogg Vorbis files: the default export format (WAV is `format: "wav"`).

use std::num::{NonZeroU8, NonZeroU32};
use std::path::Path;

use sfx_graph::Audio;
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

/// Default VBR quality (0..1; 0.5 is about 160 kbps for stereo, less for mono).
pub const DEFAULT_QUALITY: f32 = 0.5;

/// A fixed stream serial, so the same audio always gives the same bytes.
const SERIAL: i32 = 0x5f5d_0001;

/// Writes `audio` to `path` as Ogg Vorbis at VBR `quality` (0..1). The
/// stream holds exactly `audio.len()` samples per channel (the last granule
/// position), so loop points in samples stay valid.
pub fn write(path: &Path, audio: &Audio, quality: f32) -> Result<(), String> {
    if !(0.0..=1.0).contains(&quality) {
        return Err(format!("quality: {quality}: use 0..1"));
    }
    let err = |e: &dyn std::fmt::Display| format!("writing {}: {e}", path.display());
    let rate = NonZeroU32::new(audio.sample_rate).ok_or_else(|| err(&"sample rate 0"))?;
    let channels = u8::try_from(audio.channels.len().max(1))
        .ok()
        .and_then(NonZeroU8::new)
        .ok_or_else(|| err(&"too many channels"))?;
    let file = std::fs::File::create(path).map_err(|e| err(&e))?;
    let mut enc = VorbisEncoderBuilder::new_with_serial(
        rate,
        channels,
        std::io::BufWriter::new(file),
        SERIAL,
    )
    .bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
        target_quality: quality,
    })
    .build()
    .map_err(|e| err(&e))?;
    const BLOCK: usize = 4096;
    let n = audio.len();
    let mut start = 0;
    while start < n {
        let end = (start + BLOCK).min(n);
        let block: Vec<&[f32]> = audio.channels.iter().map(|c| &c[start..end]).collect();
        enc.encode_audio_block(&block).map_err(|e| err(&e))?;
        start = end;
    }
    let mut w = enc.finish().map_err(|e| err(&e))?;
    std::io::Write::flush(&mut w).map_err(|e| err(&e))
}
