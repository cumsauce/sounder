//! sounder's synthesis core.
//!
//! A sound is a [`SoundDoc`]: a JSON list of nodes (`{"type", "name", ...}`)
//! evaluated offline over whole buffers. Every node runs once over the full
//! length and yields named ports (the first is the default). Nodes run in
//! dependency levels, a level's nodes in parallel (rayon), and results are
//! memoized by a Merkle key, so a tweak re-renders only that node and what
//! reads it. The same document and seed give bit-identical samples at any
//! thread count.
//!
//! The structure is copied from modeler's material graph
//! (`src/material/{node,eval,doc}.rs`).

// DSP loops index several buffers by the same sample index; that reads
// clearer than zipped iterators.
#![allow(clippy::needless_range_loop)]

pub mod buf;
pub mod calc;
pub mod doc;
pub mod dsp;
pub mod eval;
pub mod feedback;
pub mod instance;
pub mod loudness;
pub mod macros;
pub mod node;
mod nodes;
pub mod params;
pub mod spec;
pub mod wav;

pub use buf::{Audio, Buf, Event, Sig};
pub use doc::{ApplyReport, DocEdit, Kind, RenderOpts, Rendered, SOUND_FORMAT_VERSION, SoundDoc};
pub use eval::{Ctx, Evaluated, Parsed, clear_cache, evaluate};
pub use instance::SoundSource;
pub use macros::{Claim, KnobMove, Macro, MapCurve, MapEntry, Measure};
pub use node::{In, NODE_TYPES, describe, example};
pub use params::Params;
pub use spec::{Component, Spec};

/// Converts decibels to a linear gain.
#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    // Clamped so absurd values stay finite (inf * 0 would be NaN).
    10f32.powf(db.clamp(-300.0, 300.0) / 20.0)
}

/// Converts a linear gain to decibels (floored at -200 dB).
#[inline]
pub fn gain_to_db(g: f32) -> f32 {
    20.0 * g.abs().max(1e-10).log10()
}

/// A stable 64-bit mix (splitmix64), for seeds that must not change across
/// Rust versions the way `DefaultHasher` may.
pub fn mix_seed(a: u64, b: u64) -> u64 {
    let mut z = a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// FNV-1a of a string, for mixing node names into seeds.
pub fn hash_str(s: &str) -> u64 {
    s.bytes().fold(0xCBF2_9CE4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01B3)
    })
}
