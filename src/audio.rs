use crate::library::Track;
use cpal::traits::{DeviceTrait, HostTrait};
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};

pub enum Command {
    Load {
        track: Track,
        device: String,
        native: bool,
        exclusive: bool,
        strict: bool,
        ticket: u64,
    },
    Queue {
        track: Track,
        ticket: u64,
        plan: u64,
        pcm: Option<crate::streaming::Pcm>,
        exclusive: bool,
        strict: bool,
    },
    Probe(String),
    Pause,
    Resume,
    Stop,
    Seek(f64),
    Dsp(crate::dsp::Config),
    Quit,
}

// A Started notification is infrequent and owns the complete track tags. Keeping
// it inline avoids additional allocation at each gapless transition.
#[allow(clippy::large_enum_variant)]
pub enum Event {
    Loading(u64, u64),
    Started {
        ticket: u64,
        track: Track,
        output: String,
        cover: Option<crate::library::Artwork>,
        exclusive: bool,
    },
    Notice(String),
    Position(f64, bool),
    Ended,
    Error(u64, String),
    TrackError {
        ticket: u64,
        source: String,
        reason: String,
    },
}

pub struct Audio {
    thread: Option<std::thread::JoinHandle<()>>,
    pub tx: Sender<Command>,
    pub rx: Receiver<Event>,
    pub generation: Arc<AtomicU64>,
    pub gain: Arc<AtomicU32>,
    pub plan: Arc<AtomicU64>,
    pub meters: Arc<[AtomicU32; 2]>,
    pub dsd_dop: Arc<AtomicBool>,
}

impl Audio {
    pub fn new(cache: PathBuf, auth: crate::network::AuthStore) -> Self {
        let (tx, commands) = mpsc::channel();
        let (events, rx) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let plan = Arc::new(AtomicU64::new(0));
        let p = plan.clone();
        let gain = Arc::new(AtomicU32::new(0.25f32.to_bits()));
        let meters = Arc::new([AtomicU32::new(0), AtomicU32::new(0)]);
        let dsd_dop = Arc::new(AtomicBool::new(false));
        let dop = dsd_dop.clone();
        let (g, v, m) = (generation.clone(), gain.clone(), meters.clone());
        let thread = std::thread::spawn(move || {
            worker(
                commands,
                events,
                cache,
                SharedState {
                    generation: g,
                    gain: v,
                    meters: m,
                    plan_epoch: p,
                    dsd_dop: dop,
                },
                auth,
            )
        });
        Self {
            thread: Some(thread),
            tx,
            rx,
            generation,
            gain,
            plan,
            meters,
            dsd_dop,
        }
    }
    pub fn load(
        &self,
        track: Track,
        device: String,
        native: bool,
        exclusive: bool,
        strict: bool,
    ) -> u64 {
        let ticket = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.tx.send(Command::Load {
            track,
            device,
            native,
            exclusive,
            strict,
            ticket,
        });
        ticket
    }
    pub fn plan_next(&self) {
        self.plan.fetch_add(1, Ordering::Relaxed);
    }
    pub fn queue(
        &self,
        track: Track,
        ticket: u64,
        plan: u64,
        pcm: Option<crate::streaming::Pcm>,
        exclusive: bool,
        strict: bool,
    ) {
        let _ = self.tx.send(Command::Queue {
            track,
            ticket,
            plan,
            pcm,
            exclusive,
            strict,
        });
    }
    pub fn stop(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(Command::Stop);
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        let _ = self.tx.send(Command::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn device_label(device: &cpal::Device) -> Option<String> {
    let description = device.description().ok()?;
    #[cfg(target_os = "linux")]
    if let Some(driver) = description.driver() {
        return Some(format!("{} · {driver}", description.name()));
    }
    Some(description.name().to_owned())
}
pub fn devices() -> Vec<String> {
    cpal::default_host()
        .output_devices()
        .into_iter()
        .flatten()
        .filter_map(|d| device_label(&d))
        .collect()
}

fn output(
    device_name: &str,
    native_rate: Option<rodio::SampleRate>,
    errors: Sender<Event>,
    ticket: u64,
) -> Result<MixerDeviceSink, String> {
    let host = cpal::default_host();
    let device = if device_name.is_empty() {
        host.default_output_device()
    } else {
        host.output_devices().ok().and_then(|mut ds| {
            ds.find(|d| device_label(d).is_some_and(|label| label == device_name))
        })
    }
    .ok_or("Uscita audio non disponibile. Aggiorna l'elenco dei dispositivi")?;
    let mut builder = DeviceSinkBuilder::from_device(device)
        .map_err(|e| format!("Configurazione uscita: {e}"))?;
    if let Some(rate) = native_rate {
        builder = builder.with_sample_rate(rate);
    }
    builder.with_error_callback(move |_| { let _ = errors.send(Event::Error(ticket, "Il dispositivo audio è stato scollegato o ha interrotto lo stream".into())); })
        .open_stream().map_err(|_| "Formato non accettato dal DAC. Disattiva Segui frequenza sorgente o scegli un'altra uscita".into())
}

pub fn cover(path: &Path) -> Option<Vec<u8>> {
    use lofty::file::TaggedFileExt;
    let file = lofty::probe::Probe::open(path)
        .ok()?
        .guess_file_type()
        .ok()?
        .read()
        .ok()?;
    use lofty::picture::PictureType;
    let picture = file
        .tags()
        .iter()
        .flat_map(|tag| tag.pictures())
        .filter(|p| p.data().len() <= 8 * 1024 * 1024)
        .min_by_key(|p| {
            if p.pic_type() == PictureType::CoverFront {
                0
            } else {
                1
            }
        })?;
    Some(picture.data().to_vec())
}

struct SharedState {
    generation: Arc<AtomicU64>,
    gain: Arc<AtomicU32>,
    meters: Arc<[AtomicU32; 2]>,
    plan_epoch: Arc<AtomicU64>,
    dsd_dop: Arc<AtomicBool>,
}
fn worker(
    commands: Receiver<Command>,
    events: Sender<Event>,
    root: PathBuf,
    state: SharedState,
    auth: crate::network::AuthStore,
) {
    let SharedState {
        generation,
        gain,
        meters,
        plan_epoch,
        dsd_dop,
    } = state;
    let mut dsp = crate::dsp::Config::default();
    let mut queued_plan = None;
    let mut session_native = true;
    let mut session_exclusive = false;
    let mut session_strict = false;
    let mut session_channels = 0;
    let mut session_description = String::new();
    let mut stream: Option<MixerDeviceSink> = None;
    let mut player: Option<Player> = None;
    let mut active = false;
    let mut hal: Option<crate::exclusive::HalStream> = None;
    let mut hal_tracks = std::collections::HashMap::<u64, Track>::new();
    let mut hal_token = 1;
    let mut session_ticket = 0;
    let mut verified_at = std::time::Instant::now();
    loop {
        match commands.recv_timeout(Duration::from_millis(60)) {
            Ok(Command::Dsp(config)) => dsp = config,
            Ok(Command::Quit) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(Command::Load {
                mut track,
                device,
                native,
                exclusive,
                strict,
                ticket,
            }) => {
                if generation.load(Ordering::Relaxed) != ticket {
                    continue;
                }
                if let Some(old) = player.take() {
                    old.stop();
                }
                stream.take();
                hal.take();
                hal_tracks.clear();
                session_ticket = ticket;
                active = false;
                queued_plan = None;
                session_native = native;
                session_exclusive = exclusive;
                session_strict = strict;
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let decode_mode = if dsd_dop.load(Ordering::Relaxed)
                        && crate::decoder::is_dsd(&track.source)
                    {
                        if !exclusive {
                            return Err(
                                "DoP requires an exclusive integer output and a compatible DSD DAC"
                                    .into(),
                            );
                        }
                        crate::decoder::Mode::DoP
                    } else {
                        crate::decoder::Mode::from(exclusive && strict)
                    };
                    let mut prepared = crate::streaming::Stream::prepare(
                        &mut track,
                        &root,
                        &auth,
                        crate::streaming::Cancellation::new(generation.clone(), ticket, None),
                        decode_mode,
                        Some(events.clone()),
                    )?;
                    let art = prepared.cover.take();
                    if exclusive {
                        let pcm = crate::streaming::Pcm::from(prepared);
                        let dop = pcm.transport == crate::integer::Transport::DoP;
                        let mut exclusive_stream = crate::exclusive::HalStream::open(
                            &device,
                            pcm,
                            plan_epoch.clone(),
                            strict,
                        )?;
                        let description = format!(
                            "Esclusiva {} · {} Hz · {} bit · {} ch{}",
                            if dop {
                                "DSD / DoP"
                            } else if exclusive_stream.format.floating {
                                "float"
                            } else {
                                "integer"
                            },
                            exclusive_stream.format.rate,
                            exclusive_stream.format.bits,
                            exclusive_stream.format.channels,
                            if strict {
                                " · rigoroso, 0 dB"
                            } else {
                                " · volume hardware, bit-perfect non certificato"
                            }
                        );
                        if generation.load(Ordering::Relaxed) != ticket {
                            return Err("Caricamento annullato".into());
                        }
                        exclusive_stream.play()?;
                        session_description = description.clone();
                        hal_token = 1;
                        hal = Some(exclusive_stream);
                        let _ = events.send(Event::Started {
                            ticket,
                            track: track.clone(),
                            output: description,
                            cover: art,
                            exclusive: true,
                        });
                        return Ok(None);
                    }
                    let gain_tags = if dsp.mode == crate::dsp::ReplayGain::Off {
                        prepared.gain
                    } else {
                        prepared.replay_gain(&track.source, &root, &auth)
                    };
                    let factor = dsp.factor(gain_tags);
                    let decoder = crate::streaming::Shared::new(prepared);
                    if generation.load(Ordering::Relaxed) != ticket {
                        return Err("Caricamento annullato".into());
                    }
                    let mut sink = output(
                        &device,
                        native.then(|| decoder.sample_rate()),
                        events.clone(),
                        ticket,
                    )?;
                    sink.log_on_drop(false);
                    let cfg = sink.config();
                    let description = format!(
                        "{} · {:.1} kHz · {} · {} ch",
                        if device.is_empty() {
                            "Uscita di sistema"
                        } else {
                            &device
                        },
                        cfg.sample_rate().get() as f64 / 1000.0,
                        cfg.sample_format(),
                        cfg.channel_count()
                    );
                    let p = Player::connect_new(sink.mixer());
                    p.pause();
                    p.append(
                        Metered::new(decoder, gain.clone(), meters.clone()).with_factor(factor),
                    );
                    Ok(Some((sink, p, description, art)))
                }))
                .unwrap_or_else(|_| Err("Il decoder ha interrotto il caricamento del file".into()));
                match result {
                    Ok(Some((sink, p, description, art)))
                        if generation.load(Ordering::Relaxed) == ticket =>
                    {
                        session_channels = u16::from(track.channels);
                        session_description = description.clone();
                        p.play();
                        player = Some(p);
                        stream = Some(sink);
                        active = true;
                        let _ = events.send(Event::Started {
                            ticket,
                            track,
                            output: description,
                            cover: art,
                            exclusive: false,
                        });
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let _ = events.send(Event::TrackError {
                            ticket,
                            source: track.source.clone(),
                            reason: e,
                        });
                    }
                }
            }
            Ok(Command::Queue {
                mut track,
                ticket,
                plan,
                pcm,
                exclusive,
                strict,
            }) => {
                if generation.load(Ordering::Relaxed) != ticket
                    || plan_epoch.load(Ordering::Relaxed) != plan
                    || queued_plan == Some(plan)
                {
                    continue;
                }
                if exclusive != session_exclusive || strict != session_strict {
                    continue;
                }
                if let Some(h) = &mut hal {
                    let result = pcm
                        .ok_or_else(|| "PCM integer successivo non ancora preparato".to_string())
                        .and_then(|pcm| {
                            if let crate::streaming::Pcm::Streaming(ref stream) = pcm {
                                stream.set_events(events.clone())?;
                            }
                            track.rate = pcm.rate;
                            track.channels = pcm.channels as u8;
                            if let Some(duration) = pcm.duration() {
                                track.duration = duration;
                            }
                            h.queue(pcm, plan)
                        });
                    match result {
                        Ok(token) => {
                            hal_tracks.insert(token, track);
                            queued_plan = Some(plan);
                            let _ = events.send(Event::Notice(
                                "Gapless esclusivo: prossimo brano pronto".into(),
                            ));
                        }
                        Err(e) => {
                            let _ = events.send(Event::Notice(e));
                        }
                    }
                    continue;
                }
                if let (Some(p), Some(sink)) = (&player, &stream) {
                    let result = (|| -> Result<(), String> {
                        let mut prepared = match pcm {
                            Some(crate::streaming::Pcm::Streaming(stream)) => *stream,
                            Some(crate::streaming::Pcm::Memory(_)) | None => {
                                crate::streaming::Stream::prepare(
                                    &mut track,
                                    &root,
                                    &auth,
                                    crate::streaming::Cancellation::new(
                                        generation.clone(),
                                        ticket,
                                        Some((plan_epoch.clone(), plan)),
                                    ),
                                    false,
                                    Some(events.clone()),
                                )?
                            }
                        };
                        prepared.set_events(events.clone())?;
                        track.rate = prepared.info.rate;
                        track.channels = prepared.info.channels as u8;
                        if session_native
                            && (track.rate != sink.config().sample_rate().get()
                                || u16::from(track.channels) != session_channels)
                        {
                            let _=events.send(Event::Notice("Cambio formato: il prossimo brano richiede la riapertura dell'uscita".into()));
                            return Ok(());
                        }
                        let gain_tags = if dsp.mode == crate::dsp::ReplayGain::Off {
                            prepared.gain
                        } else {
                            prepared.replay_gain(&track.source, &root, &auth)
                        };
                        let event = Event::Started {
                            ticket,
                            track,
                            output: session_description.clone(),
                            cover: prepared.cover.take(),
                            exclusive: false,
                        };
                        let factor = dsp.factor(gain_tags);
                        let source = Metered::new(
                            crate::streaming::Shared::new(prepared),
                            gain.clone(),
                            meters.clone(),
                        )
                        .with_factor(factor);
                        p.append(MarkOnStart {
                            source,
                            event: Some(event),
                            events: events.clone(),
                            plan,
                            plan_epoch: plan_epoch.clone(),
                            ticket,
                            generation: generation.clone(),
                            started: false,
                        });
                        let _ = events.send(Event::Notice(
                            "Gapless: prossimo brano accodato nello stesso stream".into(),
                        ));
                        Ok(())
                    })();
                    queued_plan = Some(plan);
                    if let Err(e) = result {
                        let _ = events.send(Event::Notice(format!(
                            "Precaricamento audio non riuscito: {e}"
                        )));
                    }
                }
            }
            Ok(Command::Probe(name)) => {
                let result = crate::exclusive::probe(&name)
                    .unwrap_or_else(|e| format!("Esclusiva non disponibile: {e}"));
                let _ = events.send(Event::Notice(result));
            }
            Ok(Command::Pause) => {
                if let Some(h) = &mut hal
                    && let Err(e) = h.pause()
                {
                    let _ = events.send(Event::Error(session_ticket, e));
                }
                if let Some(p) = &player {
                    p.pause();
                }
            }
            Ok(Command::Resume) => {
                if let Some(h) = &mut hal
                    && let Err(e) = h.play()
                {
                    let _ = events.send(Event::Error(session_ticket, e));
                }
                if let Some(p) = &player {
                    p.play();
                }
            }
            Ok(Command::Stop) => {
                hal.take();
                hal_tracks.clear();
                if let Some(p) = player.take() {
                    p.stop();
                }
                stream.take();
                active = false;
            }
            Ok(Command::Seek(pos)) => {
                if let Some(h) = &mut hal
                    && pos.is_finite()
                    && pos >= 0.0
                    && let Err(e) = h.seek(pos)
                {
                    let _ = events.send(Event::Error(session_ticket, e));
                }
                if pos.is_finite()
                    && pos >= 0.0
                    && let Some(p) = &player
                    && p.try_seek(Duration::from_secs_f64(pos)).is_err()
                {
                    let _ = events.send(Event::Error(
                        0,
                        "Seek non disponibile per questo file".into(),
                    ));
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let mut hal_error = None;
        let mut hal_ended = false;
        if let Some(h) = &mut hal {
            for (target, peak) in meters.iter().zip(&h.telemetry.peaks) {
                target.store(peak.load(Ordering::Relaxed), Ordering::Relaxed);
            }
            let token = h.telemetry.token.load(Ordering::Acquire);
            if token != hal_token {
                hal_token = token;
                if let Some(track) = hal_tracks.remove(&token) {
                    let _ = events.send(Event::Started {
                        ticket: session_ticket,
                        track,
                        output: session_description.clone(),
                        cover: None,
                        exclusive: true,
                    });
                }
            }
            for retired in h.collect_retired() {
                hal_tracks.remove(&retired);
            }
            let _ = events.send(Event::Position(
                h.telemetry.frame.load(Ordering::Relaxed) as f64 / f64::from(h.format.rate),
                h.paused,
            ));
            if h.telemetry.fault.load(Ordering::Relaxed) {
                hal_error = Some("Buffer HAL inatteso: uscita esclusiva interrotta".into());
            }
            if verified_at.elapsed() >= Duration::from_millis(500) {
                verified_at = std::time::Instant::now();
                if let Err(e) = h.verify() {
                    hal_error = Some(e);
                }
            }
            hal_ended = h.telemetry.ended.load(Ordering::Acquire);
        }
        if let Some(error) = hal_error {
            hal.take();
            hal_tracks.clear();
            let _ = events.send(Event::Error(session_ticket, error));
        } else if hal_ended {
            hal.take();
            hal_tracks.clear();
            let _ = events.send(Event::Ended);
        }
        if let Some(p) = &player {
            let _ = events.send(Event::Position(p.get_pos().as_secs_f64(), p.is_paused()));
            if active && p.empty() {
                active = false;
                let _ = events.send(Event::Ended);
            }
        }
    }
}

/// Post-gain digital peak telemetry, never simulated; no locks or allocations in next().
pub struct Metered<S> {
    factor: f32,
    source: S,
    gain: Arc<AtomicU32>,
    meters: Arc<[AtomicU32; 2]>,
    channel: usize,
    count: usize,
    peaks: [f32; 2],
}
impl<S: Source> Metered<S> {
    fn with_factor(mut self, factor: f32) -> Self {
        self.factor = factor;
        self
    }
    fn new(source: S, gain: Arc<AtomicU32>, meters: Arc<[AtomicU32; 2]>) -> Self {
        Self {
            factor: 1.0,
            source,
            gain,
            meters,
            channel: 0,
            count: 0,
            peaks: [0.0; 2],
        }
    }
}
impl<S: Source> Iterator for Metered<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let raw = self.source.next()?;
        let raw = if raw.is_finite() { raw } else { 0.0 };
        let value = raw * f32::from_bits(self.gain.load(Ordering::Relaxed)) * self.factor;
        if self.channel < 2 {
            self.peaks[self.channel] = self.peaks[self.channel].max(value.abs());
        }
        self.channel = (self.channel + 1) % usize::from(self.source.channels().get());
        self.count += 1;
        if self.count >= 1024 && self.channel == 0 {
            if self.source.channels().get() == 1 {
                self.peaks[1] = self.peaks[0];
            }
            for (meter, peak) in self.meters.iter().zip(self.peaks) {
                meter.store(peak.to_bits(), Ordering::Relaxed);
            }
            self.peaks = [0.0; 2];
            self.count = 0;
        }
        Some(value)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.source.size_hint()
    }
}
impl<S: Source> Source for Metered<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.source.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.source.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.source.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.channel = 0;
        self.count = 0;
        self.peaks = [0.0; 2];
        self.source.try_seek(pos)
    }
}

/// A queued track starts only when its first actual sample is requested by the mixer.
/// Plan/generation gates discard stale queue entries without restarting the current sound.
struct MarkOnStart<S> {
    source: S,
    event: Option<Event>,
    events: Sender<Event>,
    plan: u64,
    plan_epoch: Arc<AtomicU64>,
    ticket: u64,
    generation: Arc<AtomicU64>,
    started: bool,
}
impl<S: Source> Iterator for MarkOnStart<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if !self.started
            && (self.plan != self.plan_epoch.load(Ordering::Relaxed)
                || self.ticket != self.generation.load(Ordering::Relaxed))
        {
            return None;
        }
        let sample = self.source.next()?;
        if !self.started {
            self.started = true;
            if let Some(event) = self.event.take() {
                let _ = self.events.send(event);
            }
        }
        Some(sample)
    }
}
impl<S: Source> Source for MarkOnStart<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.source.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.source.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.source.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.source.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_track_gain_scales_shared_samples_and_default_is_transparent() {
        use rodio::buffer::SamplesBuffer;
        let input = vec![0.5_f32, -0.25, 0.125, -0.0625];
        let master = Arc::new(AtomicU32::new(1.0_f32.to_bits()));
        let meters = Arc::new([AtomicU32::new(0), AtomicU32::new(0)]);
        let source = || {
            SamplesBuffer::new(
                2.try_into().unwrap(),
                44100.try_into().unwrap(),
                input.clone(),
            )
        };
        assert_eq!(
            Metered::new(source(), master.clone(), meters.clone()).collect::<Vec<_>>(),
            input
        );
        let processed = Metered::new(source(), master, meters)
            .with_factor(0.5)
            .collect::<Vec<_>>();
        assert_eq!(
            processed,
            input.into_iter().map(|v| v * 0.5).collect::<Vec<_>>()
        );
    }

    #[test]
    fn gain_and_stereo_meter_use_real_samples() {
        let gain = Arc::new(AtomicU32::new(0.5f32.to_bits()));
        let meters = Arc::new([AtomicU32::new(0), AtomicU32::new(0)]);
        let samples: Vec<f32> = (0..1024).flat_map(|_| [0.8, -0.4]).collect();
        let source = rodio::buffer::SamplesBuffer::new(
            rodio::ChannelCount::new(2).unwrap(),
            rodio::SampleRate::new(44100).unwrap(),
            samples,
        );
        let mut measured = Metered::new(source, gain, meters.clone());
        assert_eq!(measured.by_ref().next(), Some(0.4));
        measured.for_each(drop);
        assert!((f32::from_bits(meters[0].load(Ordering::Relaxed)) - 0.4).abs() < 0.00001);
        assert!((f32::from_bits(meters[1].load(Ordering::Relaxed)) - 0.2).abs() < 0.00001);
    }
}

#[cfg(test)]
mod native_test {
    use super::*;
    #[test]
    #[ignore = "opens the physical default output briefly; run explicitly"]
    fn native_play_pause_seek_and_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("quiet-fixture.wav");
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut file = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..44100 * 3 {
            let sample =
                ((i as f32 * std::f32::consts::TAU * 440.0 / 44100.0).sin() * 300.0) as i16;
            file.write_sample(sample).unwrap();
            file.write_sample(sample).unwrap();
        }
        file.finalize().unwrap();
        let audio = Audio::new(dir.path().join("cache"), Default::default());
        audio.gain.store(0.05f32.to_bits(), Ordering::Relaxed);
        audio.load(
            Track::read(path.to_string_lossy().into_owned()),
            String::new(),
            true,
            false,
            false,
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut started = false;
        let mut paused = false;
        let mut resumed = false;
        let mut ended = false;
        while std::time::Instant::now() < deadline {
            match audio.rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                Event::Started { track, output, .. } => {
                    assert_eq!(track.rate, 44100);
                    println!("Native output: {output}");
                    started = true;
                }
                Event::Position(pos, false) if started && !paused && pos > 0.15 => {
                    audio.tx.send(Command::Pause).unwrap();
                }
                Event::Position(_, true) if !paused => {
                    paused = true;
                    audio.tx.send(Command::Seek(2.0)).unwrap();
                    audio.tx.send(Command::Resume).unwrap();
                }
                Event::Position(pos, false) if paused && pos >= 2.0 => resumed = true,
                Event::Ended => {
                    ended = true;
                    break;
                }
                Event::Error(_, e) | Event::TrackError { reason: e, .. } => panic!("{e}"),
                _ => {}
            }
        }
        assert!(started && paused && resumed && ended);
    }
}

#[cfg(test)]
mod gapless_tests {
    use super::*;
    #[test]
    fn repeated_plan_changes_skip_stale_sources_without_cutting_the_current_samples() {
        let (input, mut output) = rodio::queue::queue(false);
        let samples = |values| {
            rodio::buffer::SamplesBuffer::new(
                rodio::ChannelCount::new(2).unwrap(),
                rodio::SampleRate::new(44100).unwrap(),
                values,
            )
        };
        let first = vec![0.1f32; 400];
        let last = vec![0.2f32; 200];
        input.append(samples(first.clone()));
        let plan = Arc::new(AtomicU64::new(1));
        let generation = Arc::new(AtomicU64::new(7));
        let (events, rx) = mpsc::channel();
        for token in 1..=9 {
            plan.store(token, Ordering::Relaxed);
            input.append(MarkOnStart {
                source: samples(if token == 9 {
                    last.clone()
                } else {
                    vec![-1.0; 200]
                }),
                event: Some(Event::Notice(token.to_string())),
                events: events.clone(),
                plan: token,
                plan_epoch: plan.clone(),
                ticket: 7,
                generation: generation.clone(),
                started: false,
            });
        }
        let expected: Vec<_> = first.into_iter().chain(last).collect();
        assert_eq!(output.by_ref().collect::<Vec<_>>(), expected);
        assert!(matches!(rx.try_recv(),Ok(Event::Notice(value)) if value=="9"));
        assert!(rx.try_recv().is_err());
    }
    #[test]
    fn shared_queue_concatenates_pcm_without_padding_and_emits_start_at_boundary() {
        let (input, mut output) = rodio::queue::queue(false);
        let samples = |v: Vec<f32>| {
            rodio::buffer::SamplesBuffer::new(
                rodio::ChannelCount::new(2).unwrap(),
                rodio::SampleRate::new(44100).unwrap(),
                v,
            )
        };
        let first: Vec<f32> = (1..=146).map(|i| i as f32 / 1000.0).collect();
        let second: Vec<f32> = (147..=364).map(|i| -(i as f32 / 1000.0)).collect();
        let (events, rx) = mpsc::channel();
        input.append(samples(first.clone()));
        input.append(MarkOnStart {
            source: samples(second.clone()),
            event: Some(Event::Notice("boundary".into())),
            events,
            plan: 1,
            plan_epoch: Arc::new(AtomicU64::new(1)),
            ticket: 1,
            generation: Arc::new(AtomicU64::new(1)),
            started: false,
        });
        let before: Vec<_> = output.by_ref().take(first.len()).collect();
        assert_eq!(before, first);
        assert!(rx.try_recv().is_err());
        let after: Vec<_> = output.collect();
        assert_eq!(after, second);
        assert!(matches!(rx.try_recv().unwrap(), Event::Notice(_)));
    }
}

#[cfg(test)]
mod live_gapless_test {
    use super::*;
    #[test]
    #[ignore = "uses BlackHole loopback explicitly, does not change default output"]
    fn native_shared_gapless_worker_uses_queued_transition() {
        let device = devices()
            .into_iter()
            .find(|n| n == "BlackHole 2ch")
            .expect("BlackHole fixture output missing");
        let dir = tempfile::tempdir().unwrap();
        let mut tracks = Vec::new();
        for name in ["first.wav", "second.wav"] {
            let path = dir.path().join(name);
            let mut wav = hound::WavWriter::create(
                &path,
                hound::WavSpec {
                    channels: 2,
                    sample_rate: 44100,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for i in 0..44100 {
                let s = if i % 2 == 0 { 17i16 } else { -17 };
                wav.write_sample(s).unwrap();
                wav.write_sample(s).unwrap();
            }
            wav.finalize().unwrap();
            tracks.push(Track::read(path.to_string_lossy().into_owned()));
        }
        for exclusive in [false, true] {
            let audio = Audio::new(dir.path().join("cache"), Default::default());
            let ticket = audio.load(tracks[0].clone(), device.clone(), true, exclusive, false);
            let mut starts = 0;
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            let mut ended = false;
            while std::time::Instant::now() < deadline {
                match audio.rx.recv_timeout(Duration::from_secs(2)).unwrap() {
                    Event::Started { track, output, .. } => {
                        assert_eq!(output.starts_with("Esclusiva "), exclusive);
                        assert_eq!(track.source, tracks[starts].source);
                        starts += 1;
                        if starts == 1 {
                            audio.plan_next();
                            let mut next_track = tracks[1].clone();
                            let prepared = crate::streaming::Stream::prepare(
                                &mut next_track,
                                &dir.path().join("cache"),
                                &Default::default(),
                                crate::streaming::Cancellation::new(
                                    audio.generation.clone(),
                                    ticket,
                                    Some((audio.plan.clone(), audio.plan.load(Ordering::Relaxed))),
                                ),
                                false,
                                None,
                            )
                            .unwrap();
                            let pcm = Some(prepared.into());
                            audio.queue(
                                tracks[1].clone(),
                                ticket,
                                audio.plan.load(Ordering::Relaxed),
                                pcm,
                                exclusive,
                                false,
                            );
                        }
                    }
                    Event::Ended => {
                        assert_eq!(starts, 2, "must not end the stream between queued tracks");
                        ended = true;
                        break;
                    }
                    Event::Error(_, e) | Event::TrackError { reason: e, .. } => panic!("{e}"),
                    _ => {}
                }
            }
            assert!(ended, "gapless worker timeout (exclusive={exclusive})");
        }
    }
}
