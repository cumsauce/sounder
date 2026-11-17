//! sounder's senses. The agent cannot hear: it reads these numbers and
//! images instead, and the human's comments are the only real ears.
//!
//! - [`descriptors`]: loudness, timing, onsets, pitch, spectral shape, the
//!   octave-band x time table, modes, stereo.
//! - [`image`]: the `listen` PNG (spectrogram, waveform, loudness).
//! - [`checks`]: `verify` / `expect` PASS / FAIL lines.
//!
//! None of it needs a window or an audio device.

// DSP loops index several buffers by the same sample index; that reads
// clearer than zipped iterators.
#![allow(clippy::needless_range_loop)]

pub mod checks;
pub mod descriptors;
pub mod image;
pub mod pitch;
pub mod raster;
pub mod stft;

pub use checks::{CheckCx, CheckResult, Expect, report, run};
pub use descriptors::{Descriptors, describe};
pub use image::{Column, View, listen_png};
