//! What flows between nodes.

use std::sync::Arc;

/// One event of an event stream (phase 3 event nodes make these).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    /// Seconds from the start.
    pub t: f64,
    pub value: f32,
    pub id: u32,
}

/// A node output over the whole sound.
#[derive(Debug, Clone)]
pub enum Buf {
    Audio(Vec<f32>),
    Stereo(Vec<[f32; 2]>),
    Events(Vec<Event>),
}

impl Buf {
    pub fn kind(&self) -> &'static str {
        match self {
            Buf::Audio(_) => "mono audio",
            Buf::Stereo(_) => "stereo audio",
            Buf::Events(_) => "events",
        }
    }

    /// Samples (audio) or events.
    pub fn len(&self) -> usize {
        match self {
            Buf::Audio(v) => v.len(),
            Buf::Stereo(v) => v.len(),
            Buf::Events(v) => v.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Heap bytes, for the memo cache's budget.
    pub fn bytes(&self) -> usize {
        match self {
            Buf::Audio(v) => v.len() * 4,
            Buf::Stereo(v) => v.len() * 8,
            Buf::Events(v) => v.len() * std::mem::size_of::<Event>(),
        }
    }

    /// From one or two channels.
    pub fn from_channels(mut ch: Vec<Vec<f32>>) -> Buf {
        if ch.len() >= 2 {
            let r = ch.swap_remove(1);
            let l = ch.swap_remove(0);
            Buf::Stereo(l.into_iter().zip(r).map(|(a, b)| [a, b]).collect())
        } else {
            Buf::Audio(ch.pop().unwrap_or_default())
        }
    }
}

/// A node's input as read: a constant, or another node's buffer.
#[derive(Debug, Clone)]
pub enum Sig {
    Const(f32),
    /// A constant stereo pair (`[l, r]` in the JSON).
    ConstStereo([f32; 2]),
    Buf(Arc<Buf>),
}

impl Sig {
    pub fn is_stereo(&self) -> bool {
        matches!(self, Sig::ConstStereo(_))
            || matches!(self, Sig::Buf(b) if matches!(**b, Buf::Stereo(_)))
    }

    pub fn is_const(&self) -> bool {
        matches!(self, Sig::Const(_) | Sig::ConstStereo(_))
    }

    /// The mono value at sample `i` (stereo reads as the average; callers
    /// that need mono check with [`crate::node::Cx::mono`] first). Past the
    /// end of a buffer reads the last sample.
    #[inline]
    pub fn at(&self, i: usize) -> f32 {
        match self {
            Sig::Const(x) => *x,
            Sig::ConstStereo([l, r]) => 0.5 * (l + r),
            Sig::Buf(b) => match &**b {
                Buf::Audio(v) => v.get(i).or(v.last()).copied().unwrap_or(0.0),
                Buf::Stereo(v) => v
                    .get(i)
                    .or(v.last())
                    .map(|s| 0.5 * (s[0] + s[1]))
                    .unwrap_or(0.0),
                Buf::Events(_) => 0.0,
            },
        }
    }

    /// Channel `c` (0 or 1) at sample `i`; mono duplicates.
    #[inline]
    pub fn ch(&self, c: usize, i: usize) -> f32 {
        match self {
            Sig::Const(x) => *x,
            Sig::ConstStereo(p) => p[c.min(1)],
            Sig::Buf(b) => match &**b {
                Buf::Audio(v) => v.get(i).or(v.last()).copied().unwrap_or(0.0),
                Buf::Stereo(v) => v.get(i).or(v.last()).map(|s| s[c.min(1)]).unwrap_or(0.0),
                Buf::Events(_) => 0.0,
            },
        }
    }

    /// The constant value, when it is one (lets nodes skip per-sample work).
    pub fn constant(&self) -> Option<f32> {
        match self {
            Sig::Const(x) => Some(*x),
            _ => None,
        }
    }

    /// The signal's channels as owned buffers of `n` samples (1 or 2).
    pub fn channels(&self, n: usize) -> Vec<Vec<f32>> {
        let k = if self.is_stereo() { 2 } else { 1 };
        (0..k)
            .map(|c| (0..n).map(|i| self.ch(c, i)).collect())
            .collect()
    }
}

/// A rendered sound: one or two channels at a sample rate.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    pub sample_rate: u32,
    /// One (mono) or two (left, right) channels of equal length.
    pub channels: Vec<Vec<f32>>,
}

impl Audio {
    pub fn mono(sample_rate: u32, x: Vec<f32>) -> Audio {
        Audio {
            sample_rate,
            channels: vec![x],
        }
    }

    pub fn from_buf(sample_rate: u32, b: &Buf) -> Result<Audio, String> {
        match b {
            Buf::Audio(v) => Ok(Audio::mono(sample_rate, v.clone())),
            Buf::Stereo(v) => Ok(Audio {
                sample_rate,
                channels: vec![v.iter().map(|s| s[0]).collect(), v.iter().map(|s| s[1]).collect()],
            }),
            Buf::Events(_) => Err("an event stream is not audio: render it through voices (phase 3) or read an audio node".into()),
        }
    }

    pub fn len(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn seconds(&self) -> f32 {
        self.len() as f32 / self.sample_rate.max(1) as f32
    }

    pub fn is_stereo(&self) -> bool {
        self.channels.len() >= 2
    }

    /// The channels averaged to one.
    pub fn mix_mono(&self) -> Vec<f32> {
        match self.channels.as_slice() {
            [] => Vec::new(),
            [m] => m.clone(),
            [l, r, ..] => l.iter().zip(r).map(|(a, b)| 0.5 * (a + b)).collect(),
        }
    }

    /// Largest absolute sample over every channel.
    pub fn peak(&self) -> f32 {
        self.channels
            .iter()
            .flatten()
            .fold(0.0f32, |m, x| m.max(x.abs()))
    }

    /// Cuts every channel to `n` samples.
    pub fn truncate(&mut self, n: usize) {
        for c in &mut self.channels {
            c.truncate(n);
        }
    }
}
