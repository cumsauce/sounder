//! Playback: a device-free [`Engine`] (all the sample work, unit-tested), the
//! [`PlayerControl`] trait the MCP `play` tool and the window talk to, and
//! (behind the `audio` feature) [`Player`], a thread that owns the `cpal`
//! output stream and drives an `Engine` from the audio callback.
//!
//! The honest limit: the human hears this, the agent does not. `do_play` only
//! reports what was sent to the speakers.

use std::sync::Arc;

use sfx_graph::{Audio, Kind, RenderOpts};

use crate::session::Session;

/// What the player tells its listener (called from the player thread, never
/// from the audio callback).
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerEvent {
    /// Playback began at `pos` seconds (looping inside `span`, if given).
    Started {
        pos: f32,
        looped: bool,
        span: Option<[f32; 2]>,
    },
    /// Stopped (by `stop`, or by reaching the end) at `pos` seconds.
    Stopped { pos: f32 },
    /// The loop wrapped (at least once since the last event).
    Looped,
    /// The playhead was moved to `pos` seconds.
    Seeked { pos: f32 },
}

/// The handle tools and the window use; `Player` implements it, tests use a fake.
pub trait PlayerControl: Send + Sync {
    /// Starts `audio` at `from` seconds. `span` `[t0, t1]` is the part that
    /// plays (and loops, when `looped`); without it the whole buffer does.
    fn play(
        &self,
        audio: Arc<Audio>,
        from: f32,
        span: Option<[f32; 2]>,
        looped: bool,
    ) -> Result<(), String>;
    fn stop(&self);
    fn seek(&self, seconds: f32);
    /// Playhead in seconds (of the buffer being played).
    fn position(&self) -> f32;
    fn playing(&self) -> bool;
    /// Replaces the event listener.
    fn set_event_handler(&self, handler: Box<dyn Fn(PlayerEvent) + Send + Sync>);
    /// A name for messages ("MacBook Pro Speakers").
    fn device_name(&self) -> String {
        "the speakers".into()
    }
}

/// Hard output safety: NaN -> 0, a soft knee above 0.95, never beyond +-1.
#[inline]
pub fn limit(x: f32) -> f32 {
    if x.is_nan() {
        return 0.0;
    }
    const KNEE: f32 = 0.95;
    let a = x.abs();
    if a <= KNEE {
        x
    } else {
        let y = KNEE + (1.0 - KNEE) * ((a - KNEE) / (1.0 - KNEE)).tanh();
        y.min(1.0).copysign(x)
    }
}

/// The pure player: a buffer, a fractional play position, a window to play
/// or loop, and `fill` to write the next block.
#[derive(Default)]
pub struct Engine {
    audio: Option<Arc<Audio>>,
    /// Frames at the buffer's rate.
    pos: f64,
    /// The playable window in frames: (start, end).
    window: (f64, f64),
    looped: bool,
    playing: bool,
    finished: bool,
    loops: u64,
}

impl Engine {
    pub fn new() -> Engine {
        Engine::default()
    }

    /// Loads `audio` and starts at `from` seconds. `span` seconds restricts
    /// the window; `looped` wraps at the window's end.
    pub fn play(&mut self, audio: Arc<Audio>, from: f32, span: Option<[f32; 2]>, looped: bool) {
        let rate = audio.sample_rate.max(1) as f64;
        let len = audio.len() as f64;
        let mut window = (0.0, len);
        if let Some([a, b]) = span {
            let s = (a.max(0.0) as f64 * rate).min(len);
            let e = (b.max(0.0) as f64 * rate).min(len);
            if e > s {
                window = (s, e);
            }
        }
        self.pos = (from.max(0.0) as f64 * rate).clamp(window.0, window.1);
        if looped && self.pos >= window.1 {
            self.pos = window.0;
        }
        self.window = window;
        self.looped = looped;
        self.playing = len > 0.0 && window.1 > window.0;
        self.finished = false;
        self.loops = 0;
        self.audio = Some(audio);
    }

    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Moves the playhead (seconds), inside the window. Does not start or
    /// stop playback.
    pub fn seek(&mut self, seconds: f32) {
        let rate = self.rate() as f64;
        self.pos = (seconds.max(0.0) as f64 * rate).clamp(self.window.0, self.window.1);
        if self.looped && self.pos >= self.window.1 {
            self.pos = self.window.0;
        }
    }

    pub fn rate(&self) -> u32 {
        self.audio.as_ref().map_or(1, |a| a.sample_rate.max(1))
    }

    /// Position in frames at the buffer's rate.
    pub fn position_frames(&self) -> f64 {
        self.pos
    }

    pub fn position_seconds(&self) -> f32 {
        (self.pos / self.rate() as f64) as f32
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    /// True once a non-looping buffer has played to its end.
    pub fn finished(&self) -> bool {
        self.finished
    }

    /// `finished`, cleared: true once per end.
    pub fn take_finished(&mut self) -> bool {
        std::mem::take(&mut self.finished)
    }

    /// How many times the loop has wrapped since `play`.
    pub fn loops(&self) -> u64 {
        self.loops
    }

    pub fn looped(&self) -> bool {
        self.looped
    }

    /// The window in seconds, if it is not the whole buffer.
    pub fn span_seconds(&self) -> Option<[f32; 2]> {
        let a = self.audio.as_ref()?;
        let r = a.sample_rate.max(1) as f64;
        if self.window == (0.0, a.len() as f64) {
            None
        } else {
            Some([(self.window.0 / r) as f32, (self.window.1 / r) as f32])
        }
    }

    /// Writes the next `out.len() / channels` frames at `device_rate`.
    /// No allocation, no locking, no panics.
    pub fn fill(&mut self, out: &mut [f32], channels: usize, device_rate: u32) {
        out.fill(0.0);
        if channels == 0 || device_rate == 0 || !self.playing {
            return;
        }
        let Some(audio) = self.audio.as_ref() else {
            return;
        };
        let chans = audio.channels.as_slice();
        let len = audio.len();
        if chans.is_empty() || len == 0 {
            self.playing = false;
            return;
        }
        let step = audio.sample_rate.max(1) as f64 / device_rate as f64;
        let (start, end) = self.window;
        for frame in out.chunks_exact_mut(channels) {
            let i = self.pos as usize;
            let frac = (self.pos - i as f64) as f32;
            let next = if frac == 0.0 {
                i
            } else if self.looped && (i + 1) as f64 >= end {
                start as usize
            } else {
                (i + 1).min(len - 1)
            };
            let i = i.min(len - 1);
            let at = |c: &[f32]| {
                let a = c.get(i).copied().unwrap_or(0.0);
                let b = c.get(next).copied().unwrap_or(a);
                a + (b - a) * frac
            };
            let l = at(&chans[0]);
            if chans.len() >= 2 && channels == 1 {
                frame[0] = limit(0.5 * (l + at(&chans[1])));
            } else if chans.len() >= 2 {
                frame[0] = limit(l);
                frame[1] = limit(at(&chans[1]));
            } else {
                let v = limit(l);
                frame.fill(v);
            }
            self.pos += step;
            if self.pos >= end {
                if self.looped {
                    let span = (end - start).max(1.0);
                    self.pos = start + (self.pos - end) % span;
                    self.loops += 1;
                } else {
                    self.pos = end;
                    self.playing = false;
                    self.finished = true;
                    return;
                }
            }
        }
    }
}

/// What the `play` tool was asked for.
#[derive(Clone, Debug, Default)]
pub struct PlayRequest {
    pub doc: Option<String>,
    pub node: Option<String>,
    pub seed: Option<u64>,
    /// None: loop when the sound's kind is `loop`.
    pub looped: Option<bool>,
    pub stop: bool,
}

/// The `play` tool: renders the sound (or one node), hands it to `player`,
/// returns the one-line answer. Plain function so a fake player can test it.
pub fn do_play(
    s: &mut Session,
    req: &PlayRequest,
    player: &dyn PlayerControl,
) -> Result<String, String> {
    if req.stop {
        player.stop();
        return Ok("stopped".into());
    }
    let opts = s.render_opts();
    let salt = s.generation();
    let name = s.target_name(req.doc.as_deref())?;
    let sound = s.sound(Some(&name))?;
    let is_loop = matches!(sound.doc.kind, Kind::Loop);
    let rendered = match req.seed {
        Some(sd) => sound.doc.render(&RenderOpts {
            seed: Some(sd),
            ..opts
        })?,
        None => sound.rendered(&opts, salt)?.clone(),
    };
    let mut audio = rendered.node_audio(req.node.as_deref())?;
    if req.node.is_some() {
        audio.truncate(rendered.audio.len());
    }
    if audio.is_empty() {
        return Err("the sound renders to no samples: nothing to play".into());
    }
    let looped = req.looped.unwrap_or(is_loop);
    let secs = audio.seconds();
    player.play(Arc::new(audio), 0.0, None, looped)?;
    let what = req
        .node
        .as_deref()
        .map_or(name.clone(), |n| format!("{name}.{n}"));
    Ok(format!(
        "playing `{what}` ({secs:.2} s) on {}{}. You cannot hear it: the human does",
        player.device_name(),
        if looped { "; loop" } else { "" }
    ))
}

#[cfg(feature = "audio")]
pub use device::Player;

#[cfg(feature = "audio")]
mod device {
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SizedSample};
    use sfx_graph::Audio;

    use super::{Engine, PlayerControl, PlayerEvent};

    /// State the audio callback publishes and any thread reads.
    #[derive(Default)]
    struct Shared {
        /// `f64::to_bits` of the position in frames at the buffer's rate.
        pos_bits: AtomicU64,
        rate: AtomicU32,
        playing: AtomicBool,
        finished: AtomicBool,
        loops: AtomicU64,
    }

    impl Shared {
        fn publish(&self, e: &mut Engine) {
            self.pos_bits
                .store(e.position_frames().to_bits(), Ordering::Relaxed);
            self.rate.store(e.rate(), Ordering::Relaxed);
            self.playing.store(e.playing(), Ordering::Relaxed);
            self.loops.store(e.loops(), Ordering::Relaxed);
            if e.take_finished() {
                self.finished.store(true, Ordering::Release);
            }
        }
        fn seconds(&self) -> f32 {
            let frames = f64::from_bits(self.pos_bits.load(Ordering::Relaxed));
            (frames / self.rate.load(Ordering::Relaxed).max(1) as f64) as f32
        }
    }

    enum Cmd {
        Play {
            audio: Arc<Audio>,
            from: f32,
            span: Option<[f32; 2]>,
            looped: bool,
        },
        Stop,
        Seek(f32),
    }

    type Handler = Arc<dyn Fn(PlayerEvent) + Send + Sync>;

    struct Inner {
        tx: Mutex<mpsc::Sender<Cmd>>,
        shared: Arc<Shared>,
        handler: Arc<Mutex<Option<Handler>>>,
        device: String,
    }

    /// The speaker output: cheap to clone, `Send + Sync`. A thread owns the
    /// `cpal` stream (streams are not `Send` everywhere).
    #[derive(Clone)]
    pub struct Player(Arc<Inner>);

    impl Player {
        /// Opens the default output device. Errors are plain English.
        pub fn start() -> Result<Player, String> {
            let shared = Arc::new(Shared::default());
            let handler: Arc<Mutex<Option<Handler>>> = Arc::default();
            let (tx, rx) = mpsc::channel::<Cmd>();
            let (ready_tx, ready_rx) = mpsc::channel::<Result<String, String>>();
            let (t_shared, t_handler) = (shared.clone(), handler.clone());
            std::thread::Builder::new()
                .name("sounder-player".into())
                .spawn(move || thread_main(rx, ready_tx, t_shared, t_handler))
                .map_err(|e| format!("could not start the player thread: {e}"))?;
            let device = ready_rx
                .recv_timeout(Duration::from_secs(10))
                .map_err(|_| {
                    "no audio output device: the audio system did not answer".to_string()
                })??;
            Ok(Player(Arc::new(Inner {
                tx: Mutex::new(tx),
                shared,
                handler,
                device,
            })))
        }

        fn send(&self, c: Cmd) -> Result<(), String> {
            let tx = self
                .0
                .tx
                .lock()
                .map_err(|_| "the player is broken (poisoned lock)".to_string())?;
            tx.send(c)
                .map_err(|_| "the player thread has stopped: restart sounder".to_string())
        }
    }

    impl PlayerControl for Player {
        fn play(
            &self,
            audio: Arc<Audio>,
            from: f32,
            span: Option<[f32; 2]>,
            looped: bool,
        ) -> Result<(), String> {
            self.send(Cmd::Play {
                audio,
                from,
                span,
                looped,
            })
        }
        fn stop(&self) {
            let _ = self.send(Cmd::Stop);
        }
        fn seek(&self, seconds: f32) {
            let _ = self.send(Cmd::Seek(seconds));
        }
        fn position(&self) -> f32 {
            self.0.shared.seconds()
        }
        fn playing(&self) -> bool {
            self.0.shared.playing.load(Ordering::Relaxed)
        }
        fn set_event_handler(&self, handler: Box<dyn Fn(PlayerEvent) + Send + Sync>) {
            if let Ok(mut h) = self.0.handler.lock() {
                *h = Some(Arc::from(handler));
            }
        }
        fn device_name(&self) -> String {
            self.0.device.clone()
        }
    }

    impl Player {
        /// Same as the trait method, for callers that only have a `Player`.
        pub fn on_event(&self, handler: Box<dyn Fn(PlayerEvent) + Send + Sync>) {
            self.set_event_handler(handler);
        }
    }

    fn emit(h: &Mutex<Option<Handler>>, e: PlayerEvent) {
        let f = h.lock().ok().and_then(|g| g.clone());
        if let Some(f) = f {
            f(e);
        }
    }

    fn thread_main(
        rx: mpsc::Receiver<Cmd>,
        ready: mpsc::Sender<Result<String, String>>,
        shared: Arc<Shared>,
        handler: Arc<Mutex<Option<Handler>>>,
    ) {
        let engine = Arc::new(Mutex::new(Engine::new()));
        let (stream, device) = match open_stream(engine.clone(), shared.clone()) {
            Ok(s) => s,
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        };
        let _ = ready.send(Ok(device));
        let _keep_alive = stream;
        let mut seen_loops = 0u64;
        loop {
            match rx.recv_timeout(Duration::from_millis(15)) {
                Ok(cmd) => {
                    let Ok(mut e) = engine.lock() else { return };
                    let ev = match cmd {
                        Cmd::Play {
                            audio,
                            from,
                            span,
                            looped,
                        } => {
                            e.play(audio, from, span, looped);
                            seen_loops = 0;
                            shared.finished.store(false, Ordering::Release);
                            PlayerEvent::Started {
                                pos: e.position_seconds(),
                                looped,
                                span,
                            }
                        }
                        Cmd::Stop => {
                            let was = e.playing();
                            e.stop();
                            if !was {
                                shared.publish(&mut e);
                                continue;
                            }
                            PlayerEvent::Stopped {
                                pos: e.position_seconds(),
                            }
                        }
                        Cmd::Seek(s) => {
                            e.seek(s);
                            PlayerEvent::Seeked {
                                pos: e.position_seconds(),
                            }
                        }
                    };
                    shared.publish(&mut e);
                    drop(e);
                    emit(&handler, ev);
                }
                Err(RecvTimeoutError::Timeout) => {
                    let loops = shared.loops.load(Ordering::Relaxed);
                    if loops != seen_loops {
                        seen_loops = loops;
                        emit(&handler, PlayerEvent::Looped);
                    }
                    if shared.finished.swap(false, Ordering::AcqRel) {
                        emit(
                            &handler,
                            PlayerEvent::Stopped {
                                pos: shared.seconds(),
                            },
                        );
                    }
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn open_stream(
        engine: Arc<Mutex<Engine>>,
        shared: Arc<Shared>,
    ) -> Result<(cpal::Stream, String), String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or_else(|| {
            "no audio output device: the system reports no default output".to_string()
        })?;
        let name = device
            .description()
            .map(|d| d.name().to_string())
            .unwrap_or_else(|_| "the default output".into());
        let supported = device
            .default_output_config()
            .map_err(|e| format!("no audio output device: {name}: {e}"))?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, config, engine, shared),
            cpal::SampleFormat::I16 => build::<i16>(&device, config, engine, shared),
            cpal::SampleFormat::U16 => build::<u16>(&device, config, engine, shared),
            cpal::SampleFormat::I32 => build::<i32>(&device, config, engine, shared),
            other => Err(format!("unsupported sample format {other} on {name}")),
        }
        .map_err(|e| format!("no audio output device: {name}: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("no audio output device: {name}: {e}"))?;
        Ok((stream, name))
    }

    fn build<T>(
        device: &cpal::Device,
        config: cpal::StreamConfig,
        engine: Arc<Mutex<Engine>>,
        shared: Arc<Shared>,
    ) -> Result<cpal::Stream, String>
    where
        T: SizedSample + FromSample<f32>,
    {
        let channels = config.channels as usize;
        let rate = config.sample_rate;
        // Scratch for the f32 block; sized up front so the callback never allocates.
        let mut scratch = vec![0.0f32; 16384];
        device
            .build_output_stream(
                config,
                move |data: &mut [T], _| {
                    // Never block: on contention (a command is being applied) play silence.
                    let mut guard = engine.try_lock().ok();
                    for chunk in data.chunks_mut(scratch.len() - scratch.len() % channels.max(1)) {
                        let buf = &mut scratch[..chunk.len()];
                        match guard.as_mut() {
                            Some(e) => e.fill(buf, channels, rate),
                            None => buf.fill(0.0),
                        }
                        for (o, s) in chunk.iter_mut().zip(buf.iter()) {
                            *o = T::from_sample(*s);
                        }
                    }
                    if let Some(e) = guard.as_mut() {
                        shared.publish(e);
                    }
                },
                |e| eprintln!("sounder: audio stream: {e}"),
                None,
            )
            .map_err(|e| e.to_string())
    }
}
