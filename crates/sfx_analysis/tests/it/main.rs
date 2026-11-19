//! One test binary for the crate: a module per node family or sense.
//! See `../README.md`.

#![allow(clippy::unwrap_used)] // helpers outside #[test] fns

mod bands;
mod blast;
mod common;
mod descriptors;
mod dynamics;
mod elements;
mod expressions;
mod farnell;
mod farnell_phone;
mod farnell_rain;
mod feedback;
mod filters;
mod finish;
mod foley;
mod grains;
mod helpers;
mod layers;
mod lofi;
mod machines;
mod macros;
mod modal;
mod modfx;
mod modulators;
mod nonlinear;
mod onsets;
mod output;
mod recipes;
mod reference;
mod sequence;
mod sources;
mod space;
mod spectral;
mod surface;
mod voices;
mod water;
mod waveguide;
