//! Exclusive OS clients live on one owned thread; the shared byte renderer never mixes or resamples.
use super::*;
use std::{sync::mpsc, thread::JoinHandle, time::Duration};
#[cfg(target_os = "linux")]
#[path = "alsa.rs"]
mod backend;
#[cfg(target_os = "windows")]
#[path = "wasapi.rs"]
mod backend;

enum Control {
    Play,
    Pause,
    Seek(f64),
    Verify,
    Quit,
}
type Request = (Control, mpsc::SyncSender<Result<(), String>>);
pub struct HalStream {
    tx: mpsc::Sender<Request>,
    thread: Option<JoinHandle<()>>,
    queue: rtrb::Producer<Queued>,
    retired: rtrb::Consumer<Queued>,
    pub telemetry: Arc<Telemetry>,
    pub format: WireFormat,
    pub paused: bool,
    next_token: u64,
    transport: Transport,
}
impl HalStream {
    pub fn open(
        name: &str,
        pcm: impl Into<crate::streaming::Pcm>,
        plan: Arc<AtomicU64>,
        strict: bool,
    ) -> Result<Self, String> {
        let pcm = pcm.into();
        let transport = pcm.transport;
        let strict = strict || transport == Transport::DoP;
        if pcm.channels == 0 || pcm.channels > 8 || pcm.rate == 0 || (strict && pcm.floating) {
            return Err("Exclusive output: unsupported PCM format".into());
        }
        let (queue, pending) = rtrb::RingBuffer::new(2);
        let (retired_tx, retired) = rtrb::RingBuffer::new(32);
        let telemetry = Arc::new(Telemetry::new(1));
        let meters = telemetry.clone();
        let (tx, commands) = mpsc::channel::<Request>();
        let (ready, opened) = mpsc::sync_channel(1);
        let name = name.to_owned();
        let thread = std::thread::Builder::new()
            .name("exclusive-pcm".into())
            .spawn(move || {
                let mut device = match backend::Output::open(&name, &pcm, strict) {
                    Ok(device) => device,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                let format = device.format;
                let mut renderer = Renderer {
                    current: Queued {
                        token: 1,
                        plan: plan.load(Ordering::Relaxed),
                        pcm,
                    },
                    frame: 0,
                    pending,
                    retired: retired_tx,
                    telemetry: meters.clone(),
                    plan,
                    format,
                    empty_cycles: 0,
                    exhausted: false,
                    dop_phase: false,
                };
                if ready.send(Ok(format)).is_err() {
                    return;
                }
                let mut paused = true;
                // Reuse storage; OS writes preserve partial-buffer boundaries.
                let stride = format.bytes * format.channels;
                let capacity = (format.rate as usize / 100).max(1);
                let mut bytes = vec![0; capacity * stride];
                let mut remaining = 0;
                let mut offset = 0;
                loop {
                    let request = if paused {
                        commands.recv().ok()
                    } else {
                        match commands.recv_timeout(Duration::from_millis(3)) {
                            Ok(request) => Some(request),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    };
                    if let Some((control, reply)) = request {
                        let result = match control {
                            Control::Quit => {
                                let _ = reply.send(Ok(()));
                                break;
                            }
                            Control::Verify => device.verify(),
                            Control::Play => device.play().map(|()| paused = false),
                            Control::Pause => device.pause().map(|()| paused = true),
                            Control::Seek(seconds) => device.reset(paused).map(|()| {
                                renderer.current.pcm.seek(seconds);
                                renderer.frame =
                                    (seconds.max(0.0) * f64::from(format.rate)) as usize;
                                renderer.exhausted = false;
                                renderer.empty_cycles = 0;
                                meters.frame.store(renderer.frame as u64, Ordering::Relaxed);
                                meters.ended.store(false, Ordering::Release);
                                remaining = 0;
                                offset = 0;
                            }),
                        };
                        let _ = reply.send(result);
                    } else if paused {
                        break;
                    }
                    if paused {
                        continue;
                    }
                    let result = (|| -> Result<(), String> {
                        let available = device.available()?.min(capacity);
                        if available == 0 {
                            return Ok(());
                        }
                        if remaining == 0 {
                            remaining = available * stride;
                            offset = 0;
                            renderer.render(&mut [&mut bytes[..remaining]]);
                        }
                        let count = (remaining / stride).min(available);
                        let written =
                            device.write(&bytes[offset..offset + count * stride])? * stride;
                        if written > remaining {
                            return Err("Exclusive output returned an invalid frame count".into());
                        }
                        remaining -= written;
                        offset += written;
                        Ok(())
                    })();
                    if result.is_err() {
                        meters.fault.store(true, Ordering::Release);
                        break;
                    }
                }
            })
            .map_err(|e| format!("Exclusive output worker: {e}"))?;
        match opened
            .recv()
            .map_err(|_| "Exclusive output worker stopped")?
        {
            Ok(format) => Ok(Self {
                tx,
                thread: Some(thread),
                queue,
                retired,
                telemetry,
                format,
                paused: true,
                next_token: 2,
                transport,
            }),
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }
    fn command(&self, command: Control) -> Result<(), String> {
        let (tx, rx) = mpsc::sync_channel(1);
        self.tx
            .send((command, tx))
            .map_err(|_| "Exclusive output stopped")?;
        rx.recv_timeout(Duration::from_secs(3))
            .map_err(|_| "Exclusive output did not respond")?
    }
    pub fn play(&mut self) -> Result<(), String> {
        self.command(Control::Play)?;
        self.paused = false;
        Ok(())
    }
    pub fn pause(&mut self) -> Result<(), String> {
        self.command(Control::Pause)?;
        self.paused = true;
        Ok(())
    }
    pub fn seek(&mut self, seconds: f64) -> Result<(), String> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("Invalid seek position".into());
        }
        self.command(Control::Seek(seconds))
    }
    pub fn queue(
        &mut self,
        pcm: impl Into<crate::streaming::Pcm>,
        plan: u64,
    ) -> Result<u64, String> {
        let pcm = pcm.into();
        if self.transport != pcm.transport || !self.format.supports(&pcm) {
            return Err(
                "Exclusive output: next format requires reopening the device; no resampling".into(),
            );
        }
        let token = self.next_token;
        self.queue
            .push(Queued { token, plan, pcm })
            .map_err(|_| "Exclusive preload queue full")?;
        self.next_token = self
            .next_token
            .checked_add(1)
            .ok_or("Queue counter exhausted")?;
        Ok(token)
    }
    pub fn verify(&self) -> Result<(), String> {
        if self.telemetry.fault.load(Ordering::Acquire)
            || self.thread.as_ref().is_none_or(|t| t.is_finished())
        {
            Err("Exclusive output lost or interrupted".into())
        } else {
            self.command(Control::Verify)
        }
    }
    pub fn collect_retired(&mut self) -> Vec<u64> {
        let mut tokens = Vec::new();
        while let Ok(old) = self.retired.pop() {
            tokens.push(old.token);
        }
        tokens
    }
}
impl Drop for HalStream {
    fn drop(&mut self) {
        let _ = self.command(Control::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub fn probe(name: &str) -> Result<String, String> {
    backend::probe(name)
}
