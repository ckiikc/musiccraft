//! Bounded PCM preparation for both output paths. No I/O, decoding or locks in pull().
mod remote;
use crate::{integer::DecodedPcm, library::Track, network::AuthStore};
use std::{
    io,
    ops::Deref,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

#[derive(Clone)]
pub struct Cancellation {
    generation: Arc<AtomicU64>,
    ticket: u64,
    plan: Option<(Arc<AtomicU64>, u64)>,
    stopped: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
    transferred: Arc<AtomicU64>,
    events: Arc<Mutex<Option<mpsc::Sender<crate::audio::Event>>>>,
}
impl Cancellation {
    pub fn new(
        generation: Arc<AtomicU64>,
        ticket: u64,
        plan: Option<(Arc<AtomicU64>, u64)>,
    ) -> Self {
        Self {
            generation,
            ticket,
            plan,
            stopped: Arc::new(AtomicBool::new(false)),
            started: Arc::new(AtomicBool::new(false)),
            transferred: Arc::new(AtomicU64::new(0)),
            events: Arc::new(Mutex::new(None)),
        }
    }
    pub fn cancelled(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
            || self.generation.load(Ordering::Relaxed) != self.ticket
            || (!self.started.load(Ordering::Acquire)
                && self
                    .plan
                    .as_ref()
                    .is_some_and(|(p, n)| p.load(Ordering::Relaxed) != *n))
    }
    fn transferred(&self, count: usize) {
        let bytes = self.transferred.fetch_add(count as u64, Ordering::Relaxed) + count as u64;
        if let Ok(events) = self.events.lock()
            && let Some(events) = events.as_ref()
        {
            let _ = events.send(crate::audio::Event::Loading(self.ticket, bytes));
        }
    }
    fn check(&self) -> io::Result<()> {
        if self.cancelled() {
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Preparazione audio annullata",
            ))
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy)]
struct Frame {
    samples: [i32; 8],
    epoch: u64,
}
struct State {
    epoch: AtomicU64,
    seek: AtomicU64,
    done: AtomicU64,
    failed: AtomicBool,
}
pub enum Pull {
    Frame([i32; 8]),
    Pending,
    End,
    Failed,
}

pub struct Stream {
    pub info: DecodedPcm,
    pub duration: Option<f64>,
    pub gain: crate::dsp::Tags,
    pub cover: Option<crate::library::Artwork>,
    frames: rtrb::Consumer<Frame>,
    state: Arc<State>,
    cancel: Cancellation,
}
impl Stream {
    #[cfg(test)]
    pub(crate) fn pending_fixture() -> (Self, impl Fn()) {
        let (_, frames) = rtrb::RingBuffer::new(2);
        let state = Arc::new(State {
            epoch: AtomicU64::new(0),
            seek: AtomicU64::new(0),
            done: AtomicU64::new(u64::MAX),
            failed: AtomicBool::new(false),
        });
        let end = state.clone();
        (
            Self {
                info: DecodedPcm {
                    samples: vec![],
                    floating: false,
                    rate: 44100,
                    channels: 2,
                    bits: 32,
                },
                duration: None,
                gain: Default::default(),
                cover: None,
                frames,
                state,
                cancel: Cancellation::new(Arc::new(AtomicU64::new(0)), 0, None),
            },
            move || end.done.store(0, Ordering::Release),
        )
    }
    pub fn prepare(
        track: &mut Track,
        root: &Path,
        auth: &AuthStore,
        cancel: Cancellation,
        strict: bool,
        errors: Option<mpsc::Sender<crate::audio::Event>>,
    ) -> Result<Self, String> {
        if let Ok(mut target) = cancel.events.lock() {
            *target = errors;
        }
        let source = remote::input(&track.source, root, auth, cancel.clone())?;
        let mut decoder = crate::integer::PacketDecoder::open(source, strict)?;
        let first = decoder
            .next(&|| cancel.cancelled())?
            .ok_or("Il file audio non contiene campioni")?;
        let info = decoder.info();
        track.rate = info.rate;
        track.channels = info.channels as u8;
        if let Some(bits) = decoder.source_bits {
            track.bits = bits as u8;
        }
        if let Some(duration) = decoder.duration {
            track.duration = duration;
        }
        let mut gain = crate::dsp::Tags::default();

        let revisions = decoder
            .metadata
            .take()
            .into_iter()
            .chain(decoder.format.metadata().current().cloned())
            .collect::<Vec<_>>();
        let hydrate = !track.metadata_loaded;
        for metadata in &revisions {
            use symphonia::core::meta::StandardTagKey as K;
            for tag in metadata.tags() {
                let value = tag.value.to_string();
                let n = crate::dsp::parse_number(&value);
                let descriptive = value.chars().take(512).collect::<String>();
                match tag.std_key {
                    Some(K::TrackTitle) if hydrate => track.title = descriptive,
                    Some(K::Artist) if hydrate => track.artist = descriptive,
                    Some(K::Album) if hydrate => track.album = descriptive,
                    Some(K::AlbumArtist) if hydrate => track.album_artist = descriptive,
                    Some(K::Genre) if hydrate => track.genre = descriptive,
                    Some(K::TrackNumber) if hydrate => {
                        if let Some(n) = value.split('/').next().and_then(|n| n.parse().ok()) {
                            track.number = n;
                        }
                    }
                    Some(K::DiscNumber) if hydrate => {
                        if let Some(n) = value.split('/').next().and_then(|n| n.parse().ok()) {
                            track.disc = n;
                        }
                    }
                    Some(K::Date) if hydrate => {
                        if let Some(n) = value.get(..4).and_then(|n| n.parse().ok()) {
                            track.year = Some(n);
                        }
                    }
                    Some(K::ReplayGainTrackGain) => gain.track_gain = n,
                    Some(K::ReplayGainAlbumGain) => gain.album_gain = n,
                    Some(K::ReplayGainTrackPeak) => gain.track_peak = n,
                    Some(K::ReplayGainAlbumPeak) => gain.album_peak = n,
                    _ => {}
                }
            }
        }
        let mut visuals = revisions
            .iter()
            .flat_map(|m| m.visuals())
            .filter(|v| v.data.len() <= 8 * 1024 * 1024)
            .collect::<Vec<_>>();
        visuals
            .sort_by_key(|v| v.usage != Some(symphonia::core::meta::StandardVisualKey::FrontCover));
        let cover = visuals
            .into_iter()
            .find_map(|v| crate::library::decode_art(&v.data));
        track.metadata_loaded = true;
        // FTP range reads can stall on a data connection even after playback
        // starts. Keep a larger reserve for those sources, bounded at high rates.
        let (ahead, start, max_bytes) = if remote::ftp_source(&track.source) {
            (24, 12, 64 * 1024 * 1024)
        } else {
            (6, 1, 24 * 1024 * 1024)
        };
        let capacity = (info.rate as usize * ahead)
            .min(max_bytes / std::mem::size_of::<Frame>())
            .max(2);
        let prefill = (info.rate as usize * start).min(capacity / 2);
        let (mut producer, frames) = rtrb::RingBuffer::new(capacity);
        let state = Arc::new(State {
            epoch: AtomicU64::new(0),
            seek: AtomicU64::new(0),
            done: AtomicU64::new(u64::MAX),
            failed: AtomicBool::new(false),
        });
        let worker_state = state.clone();
        let cancellation = cancel.clone();
        let (ready, waiting) = mpsc::sync_channel(1);
        let duration = decoder.duration;
        std::thread::spawn(move || {
            let mut ready = Some(ready);
            let mut epoch = 0;
            let mut block = first;
            let mut offset = 0;
            let mut produced = 0;
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    loop {
                        if cancellation.cancelled() {
                            return Ok(());
                        }
                        let requested = worker_state.epoch.load(Ordering::Acquire);
                        if requested != epoch {
                            epoch = requested;
                            let seconds = f64::from_bits(worker_state.seek.load(Ordering::Acquire));
                            decoder.seek(seconds)?;
                            block.samples.clear();
                            offset = 0;
                            produced = 0;
                        }
                        if producer.is_full() {
                            std::thread::sleep(Duration::from_millis(20));
                            continue;
                        }
                        if offset >= block.samples.len() {
                            match decoder.next(&|| cancellation.cancelled())? {
                                Some(next) => {
                                    block = next;
                                    offset = 0;
                                }
                                None => {
                                    if let Some(tx) = ready.take() {
                                        let _ = tx.send(Ok(()));
                                    }
                                    worker_state.done.store(epoch, Ordering::Release);
                                    // Remain seekable after EOF while the tail is still buffered/paused.
                                    while !cancellation.cancelled()
                                        && worker_state.epoch.load(Ordering::Acquire) == epoch
                                    {
                                        std::thread::sleep(Duration::from_millis(50));
                                    }
                                    continue;
                                }
                            }
                        }
                        let mut frame = Frame {
                            samples: [0; 8],
                            epoch,
                        };
                        frame.samples[..block.channels]
                            .copy_from_slice(&block.samples[offset..offset + block.channels]);
                        // Only this producer can fill a slot; the consumer can only free one.
                        producer
                            .push(frame)
                            .map_err(|_| "Buffer PCM non disponibile")?;
                        offset += block.channels;
                        produced += 1;
                        if produced >= prefill
                            && let Some(tx) = ready.take()
                        {
                            let _ = tx.send(Ok(()));
                        }
                    }
                }))
                .unwrap_or_else(|_| Err("Il decoder ha interrotto la preparazione audio".into()));
            if let Err(error) = result
                && !cancellation.cancelled()
            {
                worker_state.failed.store(true, Ordering::Release);
                if let Some(tx) = ready.take() {
                    let _ = tx.send(Err(error.clone()));
                } else if let Ok(target) = cancellation.events.lock()
                    && let Some(events) = target.as_ref()
                {
                    let _ = events.send(
                        if cancellation.plan.is_none()
                            || cancellation.started.load(Ordering::Acquire)
                        {
                            crate::audio::Event::Error(cancellation.ticket, error)
                        } else {
                            crate::audio::Event::Notice(format!(
                                "Precaricamento audio interrotto: {error}"
                            ))
                        },
                    );
                }
            }
        });
        loop {
            if cancel.cancelled() {
                cancel.stopped.store(true, Ordering::Release);
                return Err("Preparazione audio annullata".into());
            }
            match waiting.recv_timeout(Duration::from_millis(20)) {
                Ok(Ok(())) => break,
                Ok(Err(e)) => return Err(e),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err("Preparazione audio interrotta".into()),
            }
        }
        Ok(Self {
            info,
            duration,
            gain,
            cover,
            frames,
            state,
            cancel,
        })
    }
    pub fn replay_gain(&self, source: &str, root: &Path, auth: &AuthStore) -> crate::dsp::Tags {
        let read = remote::input(source, root, auth, self.cancel.clone())
            .ok()
            .map(|mut reader| crate::dsp::Tags::read_from(&mut reader))
            .unwrap_or_default();
        crate::dsp::Tags {
            track_gain: read.track_gain.or(self.gain.track_gain),
            album_gain: read.album_gain.or(self.gain.album_gain),
            track_peak: read.track_peak.or(self.gain.track_peak),
            album_peak: read.album_peak.or(self.gain.album_peak),
        }
    }
    pub fn set_events(&self, events: mpsc::Sender<crate::audio::Event>) -> Result<(), String> {
        *self
            .cancel
            .events
            .lock()
            .map_err(|_| "Stato decoder non disponibile")? = Some(events);
        if self.state.failed.load(Ordering::Acquire) {
            return Err("Preparazione del brano interrotta".into());
        }
        Ok(())
    }
    pub fn pull(&mut self) -> Pull {
        if self.state.failed.load(Ordering::Acquire) {
            return Pull::Failed;
        }
        let epoch = self.state.epoch.load(Ordering::Acquire);
        // A seek discards old data in bounded slices rather than doing I/O in the callback.
        for _ in 0..256 {
            match self.frames.pop() {
                Ok(frame) if frame.epoch == epoch => {
                    self.cancel.started.store(true, Ordering::Release);
                    return Pull::Frame(frame.samples);
                }
                Ok(_) => continue,
                Err(_) => {
                    return if self.state.done.load(Ordering::Acquire) == epoch {
                        Pull::End
                    } else {
                        Pull::Pending
                    };
                }
            }
        }
        Pull::Pending
    }
    pub fn seek(&mut self, seconds: f64) {
        self.state.seek.store(seconds.to_bits(), Ordering::Release);
        self.state.epoch.fetch_add(1, Ordering::AcqRel);
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        self.cancel.stopped.store(true, Ordering::Release);
    }
}

/// Memory fixtures and progressive streams share the same HAL format checks.
pub enum Pcm {
    Memory(Arc<DecodedPcm>),
    Streaming(Box<Stream>),
}
impl From<Stream> for Pcm {
    fn from(p: Stream) -> Self {
        Self::Streaming(Box::new(p))
    }
}
impl From<Arc<DecodedPcm>> for Pcm {
    fn from(p: Arc<DecodedPcm>) -> Self {
        Self::Memory(p)
    }
}
impl Deref for Pcm {
    type Target = DecodedPcm;
    fn deref(&self) -> &DecodedPcm {
        match self {
            Self::Memory(p) => p,
            Self::Streaming(p) => &p.info,
        }
    }
}
impl Pcm {
    pub fn duration(&self) -> Option<f64> {
        match self {
            Self::Memory(p) => Some(p.frames() as f64 / f64::from(p.rate)),
            Self::Streaming(p) => p.duration,
        }
    }
    pub fn pull(&mut self, position: usize) -> Pull {
        match self {
            Self::Streaming(p) => p.pull(),
            Self::Memory(p) => {
                if position >= p.frames() {
                    return Pull::End;
                }
                let mut frame = [0; 8];
                frame[..p.channels].copy_from_slice(
                    &p.samples[position * p.channels..(position + 1) * p.channels],
                );
                Pull::Frame(frame)
            }
        }
    }
    pub fn seek(&mut self, seconds: f64) {
        if let Self::Streaming(p) = self {
            p.seek(seconds);
        }
    }
}

pub struct Shared {
    pub stream: Stream,
    channel: usize,
    frame: [i32; 8],
}
impl Shared {
    pub fn new(stream: Stream) -> Self {
        Self {
            stream,
            channel: 0,
            frame: [0; 8],
        }
    }
}
impl Iterator for Shared {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.channel == 0 {
            self.frame = match self.stream.pull() {
                Pull::Frame(f) => f,
                Pull::Pending => [0; 8],
                Pull::End | Pull::Failed => return None,
            };
        }
        let sample = self.frame[self.channel];
        self.channel = (self.channel + 1) % self.stream.info.channels;
        Some(if self.stream.info.floating {
            f32::from_bits(sample as u32)
        } else {
            (sample as f64 / 2147483648.) as f32
        })
    }
}
impl rodio::Source for Shared {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> rodio::ChannelCount {
        rodio::ChannelCount::new(self.stream.info.channels as u16)
            .unwrap_or(rodio::ChannelCount::MIN)
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        rodio::SampleRate::new(self.stream.info.rate).unwrap_or(rodio::SampleRate::MIN)
    }
    fn total_duration(&self) -> Option<Duration> {
        self.stream
            .duration
            .and_then(|s| Duration::try_from_secs_f64(s).ok())
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.stream.seek(pos.as_secs_f64());
        self.channel = 0;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
