//! sounder's senses. The agent cannot hear: it reads these numbers and
//! images instead, and the human's comments are the only real ears.
//!
//! - [`descriptors`]: loudness, timing, onsets, pitch, spectral shape, the
//!   octave-band x time table, modes, stereo.
//! - [`image`]: the `listen` PNG (spectrogram, waveform, loudness).
//! - [`reference`]: compare a sound with a recording (distance, deviations).
//! - [`checks`]: `verify` / `expect` PASS / FAIL lines.
//!
//! None of it needs a window or an audio device.

// DSP loops index several buffers by the same sample index; that reads
// clearer than zipped iterators.
#![allow(clippy::needless_range_loop)]

pub mod checks;
pub mod descriptors;
pub mod image;
pub mod measure;
pub mod modal;
pub mod pitch;
pub mod raster;
pub mod reference;
pub mod stft;

pub use checks::{CheckCx, CheckResult, Expect, report, run};
pub use descriptors::{Descriptors, describe, modes_node};
pub use image::{Column, DiffColumn, View, listen_png, listen_png_diff};
pub use reference::{Comparison, compare};
