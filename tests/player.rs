//! The player's Engine (no device), `do_play` with a fake player, and one
//! ignored real-device beep.

#![allow(clippy::unwrap_used)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::{Arc, Mutex};

use serde_json::json;
use sfx_graph::{Audio, DocEdit};
use sounder::player::{Engine, PlayRequest, PlayerControl, PlayerEvent, do_play, limit};
use sounder::session::Session;

/// Counts allocations per thread, so a test can assert `fill` makes none.
struct Counting;
thread_local! { static ALLOCS: Cell<usize> = const { Cell::new(0) }; }
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|a| a.set(a.get() + 1));
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let _ = ALLOCS.try_with(|a| a.set(a.get() + 1));
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn ramp(n: usize, rate: u32) -> Arc<Audio> {
    Arc::new(Audio::mono(rate, (0..n).map(|i| i as f32).collect()))
}

fn sine(hz: f32, secs: f32, rate: u32) -> Arc<Audio> {
    let n = (secs * rate as f32) as usize;
    Arc::new(Audio::mono(
        rate,
        (0..n)
            .map(|i| (std::f32::consts::TAU * hz * i as f32 / rate as f32).sin() * 0.5)
            .collect(),
    ))
}

#[test]
fn position_advances_by_frames_consumed() {
    let mut e = Engine::new();
    e.play(ramp(1000, 1000), 0.0, None, false);
    let mut out = vec![0.0; 100];
    e.fill(&mut out, 1, 1000);
    assert_eq!(e.position_frames(), 100.0);
    e.fill(&mut out, 1, 1000);
    assert_eq!(e.position_frames(), 200.0);
    assert_eq!(out[0], limit(100.0));
    assert!(e.playing() && !e.finished());
}

#[test]
fn loop_wraps_sample_accurately() {
    let mut e = Engine::new();
    // values 0..1000 scaled so the limiter does not touch them
    let a = Arc::new(Audio::mono(
        1000,
        (0..1000).map(|i| i as f32 / 2000.0).collect(),
    ));
    e.play(a, 0.0, Some([0.2, 0.25]), true); // frames 200..250
    let mut out = vec![0.0; 120];
    e.fill(&mut out, 1, 1000);
    for (k, v) in out.iter().enumerate() {
        let want = (200 + k % 50) as f32 / 2000.0;
        assert!((v - want).abs() < 1e-6, "k={k} {v} {want}");
    }
    assert_eq!(e.loops(), 2);
    assert!(e.playing());
}

#[test]
fn seek_moves_the_playhead() {
    let mut e = Engine::new();
    e.play(ramp(1000, 1000), 0.0, None, false);
    e.seek(0.5);
    assert_eq!(e.position_frames(), 500.0);
    let mut out = vec![0.0; 1];
    e.fill(&mut out, 1, 1000);
    assert_eq!(out[0], limit(500.0));
    e.seek(99.0);
    assert_eq!(e.position_frames(), 1000.0);
}

#[test]
fn stops_at_the_end_and_stays_silent() {
    let mut e = Engine::new();
    e.play(Arc::new(Audio::mono(1000, vec![0.5; 10])), 0.0, None, false);
    let mut out = vec![1.0; 30];
    e.fill(&mut out, 1, 1000);
    assert!(out[..10].iter().all(|v| *v == 0.5));
    assert!(out[10..].iter().all(|v| *v == 0.0));
    assert!(!e.playing());
    assert!(e.finished());
    e.fill(&mut out, 1, 1000);
    assert!(out.iter().all(|v| *v == 0.0));
    assert!(e.take_finished() && !e.finished());
}

fn zero_crossing_hz(x: &[f32], rate: u32) -> f32 {
    let ups: Vec<usize> = (1..x.len())
        .filter(|&i| x[i - 1] < 0.0 && x[i] >= 0.0)
        .collect();
    let (a, b) = (ups[0], ups[ups.len() - 1]);
    (ups.len() - 1) as f32 * rate as f32 / (b - a) as f32
}

fn resample_check(src: u32, dev: u32) {
    let mut e = Engine::new();
    let a = sine(1000.0, 1.0, src);
    e.play(a, 0.0, None, false);
    let mut out = vec![0.0; (dev as usize) * 2];
    e.fill(&mut out, 1, dev);
    let played = out.iter().rposition(|v| *v != 0.0).unwrap() + 1;
    assert!(
        (played as f32 / dev as f32 - 1.0).abs() < 0.001,
        "duration {played} frames at {dev}"
    );
    let hz = zero_crossing_hz(&out[..played], dev);
    assert!((hz / 1000.0 - 1.0).abs() < 0.001, "pitch {hz}");
}

#[test]
fn resampling_keeps_pitch_and_duration() {
    resample_check(44100, 48000);
    resample_check(48000, 44100);
}

#[test]
fn mono_fills_every_channel_and_stereo_the_first_two() {
    let mut e = Engine::new();
    e.play(Arc::new(Audio::mono(100, vec![0.25; 4])), 0.0, None, false);
    let mut out = vec![9.0; 8];
    e.fill(&mut out, 2, 100);
    assert!(out[..8].iter().all(|v| *v == 0.25)); // 4 frames x 2 channels
    let mut e = Engine::new();
    e.play(Arc::new(Audio::mono(100, vec![0.25; 8])), 0.0, None, false);
    let mut out = vec![9.0; 12];
    e.fill(&mut out, 6, 100);
    assert!(out.iter().all(|v| *v == 0.25));
    let st = Audio {
        sample_rate: 100,
        channels: vec![vec![0.1; 4], vec![0.2; 4]],
    };
    let mut e = Engine::new();
    e.play(Arc::new(st), 0.0, None, false);
    let mut out = vec![9.0; 8]; // 2 frames x 4 channels
    e.fill(&mut out, 4, 100);
    assert_eq!(&out[..4], &[0.1, 0.2, 0.0, 0.0]);
}

#[test]
fn limiter_never_exceeds_one() {
    for i in -4000..=4000 {
        let x = i as f32 / 1000.0;
        let y = limit(x);
        assert!(y.abs() <= 1.0, "{x} -> {y}");
        if x.abs() <= 0.95 {
            assert_eq!(y, x);
        }
    }
    assert_eq!(limit(f32::NAN), 0.0);
    let mut e = Engine::new();
    e.play(
        Arc::new(Audio::mono(100, vec![4.0, -4.0, 0.99])),
        0.0,
        None,
        false,
    );
    let mut out = vec![0.0; 3];
    e.fill(&mut out, 1, 100);
    assert!(out.iter().all(|v| v.abs() <= 1.0));
}

#[test]
fn fill_does_not_allocate() {
    let mut e = Engine::new();
    e.play(sine(440.0, 0.5, 44100), 0.0, Some([0.1, 0.3]), true);
    let mut out = vec![0.0; 2048];
    let before = ALLOCS.with(Cell::get);
    for _ in 0..20 {
        e.fill(&mut out, 2, 48000);
    }
    assert_eq!(ALLOCS.with(Cell::get), before);
}

/// Records what it was asked to play.
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<(usize, u32, Option<[f32; 2]>, bool)>>,
    stops: Mutex<u32>,
}
impl PlayerControl for Fake {
    fn play(
        &self,
        a: Arc<Audio>,
        _from: f32,
        span: Option<[f32; 2]>,
        looped: bool,
    ) -> Result<(), String> {
        self.calls
            .lock()
            .unwrap()
            .push((a.len(), a.sample_rate, span, looped));
        Ok(())
    }
    fn stop(&self) {
        *self.stops.lock().unwrap() += 1;
    }
    fn seek(&self, _: f32) {}
    fn position(&self) -> f32 {
        0.0
    }
    fn playing(&self) -> bool {
        false
    }
    fn set_event_handler(&self, _: Box<dyn Fn(PlayerEvent) + Send + Sync>) {}
}

fn session_with_tone() -> Session {
    let mut s = Session::new(None);
    s.new_sound("tone", sfx_graph::Kind::Oneshot).unwrap();
    let ops = vec![
        json!({"type": "osc", "name": "o", "freq": 440, "amp": 0.3}),
        json!({"type": "decay", "name": "e", "time": 0.05}),
        json!({"type": "math", "name": "m", "op": "mul", "a": "o", "b": "e"}),
        json!({"type": "output", "name": "out", "input": "m"}),
    ];
    s.apply_ops(None, ops, DocEdit::default()).unwrap();
    s
}

#[test]
fn play_tool_sends_the_render_to_the_player() {
    let mut s = session_with_tone();
    let f = Fake::default();
    let msg = do_play(&mut s, &PlayRequest::default(), &f).unwrap();
    assert!(msg.contains("playing `tone`"), "{msg}");
    let msg2 = do_play(
        &mut s,
        &PlayRequest {
            node: Some("o".into()),
            looped: Some(true),
            seed: Some(3),
            ..Default::default()
        },
        &f,
    )
    .unwrap();
    assert!(msg2.contains("tone.o") && msg2.contains("loop"), "{msg2}");
    let calls = f.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].0 > 1000 && !calls[0].3);
    assert!(calls[1].3);
    drop(calls);
    do_play(
        &mut s,
        &PlayRequest {
            stop: true,
            ..Default::default()
        },
        &f,
    )
    .unwrap();
    assert_eq!(*f.stops.lock().unwrap(), 1);
    assert!(
        do_play(
            &mut s,
            &PlayRequest {
                node: Some("nope".into()),
                ..Default::default()
            },
            &f
        )
        .is_err()
    );
}

#[cfg(feature = "audio")]
#[test]
#[ignore = "needs a speaker"]
fn real_device_beep() {
    let p = sounder::player::Player::start().unwrap();
    // sine() peaks at 0.5; a 440 Hz tone at a quiet level
    let quiet = Audio::mono(
        48000,
        sine(440.0, 0.3, 48000).channels[0]
            .iter()
            .map(|v| v * 0.2)
            .collect(),
    );
    p.play(Arc::new(quiet), 0.0, None, false).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(!p.playing());
}
