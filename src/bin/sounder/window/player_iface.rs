//! What the window needs from the player: `sounder::player`'s trait and event
//! type (the window talks only to the trait), plus a `NullPlayer` for runs
//! without an audio device and a `FakePlayer` for tests.

use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

use sfx_graph::Audio;

pub use sounder::player::{PlayerControl, PlayerEvent};

/// No audio device: every `play` says so.
pub struct NullPlayer;

impl PlayerControl for NullPlayer {
    fn play(&self, _: Arc<Audio>, _: f32, _: Option<[f32; 2]>, _: bool) -> Result<(), String> {
        Err(
            "no audio output in this build or run: playback needs the `audio` feature and a device"
                .into(),
        )
    }
    fn stop(&self) {}
    fn seek(&self, _: f32) {}
    fn position(&self) -> f32 {
        0.0
    }
    fn playing(&self) -> bool {
        false
    }
    fn set_event_handler(&self, _: Box<dyn Fn(PlayerEvent) + Send + Sync>) {}
}

/// What a [`FakePlayer`] was asked to do.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq)]
pub enum FakeCall {
    Play {
        samples: usize,
        from: f32,
        span: Option<[f32; 2]>,
        looped: bool,
    },
    Stop,
    Seek(f32),
}

#[cfg(test)]
#[derive(Default)]
struct FakeState {
    calls: Vec<FakeCall>,
    playing: bool,
    pos: f32,
}

/// Records calls and emits the events a real player would.
#[cfg(test)]
#[derive(Default)]
pub struct FakePlayer {
    st: Mutex<FakeState>,
    handler: Mutex<Option<Box<dyn Fn(PlayerEvent) + Send + Sync>>>,
}

#[cfg(test)]
impl FakePlayer {
    pub fn calls(&self) -> Vec<FakeCall> {
        self.st.lock().map(|s| s.calls.clone()).unwrap_or_default()
    }

    fn emit(&self, e: PlayerEvent) {
        if let Ok(h) = self.handler.lock()
            && let Some(h) = h.as_ref()
        {
            h(e);
        }
    }
}

#[cfg(test)]
impl PlayerControl for FakePlayer {
    fn play(
        &self,
        audio: Arc<Audio>,
        from: f32,
        span: Option<[f32; 2]>,
        looped: bool,
    ) -> Result<(), String> {
        if let Ok(mut s) = self.st.lock() {
            s.calls.push(FakeCall::Play {
                samples: audio.len(),
                from,
                span,
                looped,
            });
            s.playing = true;
            s.pos = from;
        }
        self.emit(PlayerEvent::Started {
            pos: from,
            looped,
            span,
        });
        Ok(())
    }
    fn stop(&self) {
        let pos = self.position();
        if let Ok(mut s) = self.st.lock() {
            s.calls.push(FakeCall::Stop);
            s.playing = false;
        }
        self.emit(PlayerEvent::Stopped { pos });
    }
    fn seek(&self, t: f32) {
        if let Ok(mut s) = self.st.lock() {
            s.calls.push(FakeCall::Seek(t));
            s.pos = t;
        }
        self.emit(PlayerEvent::Seeked { pos: t });
    }
    fn position(&self) -> f32 {
        self.st.lock().map_or(0.0, |s| s.pos)
    }
    fn playing(&self) -> bool {
        self.st.lock().is_ok_and(|s| s.playing)
    }
    fn set_event_handler(&self, handler: Box<dyn Fn(PlayerEvent) + Send + Sync>) {
        if let Ok(mut h) = self.handler.lock() {
            *h = Some(handler);
        }
    }
}
