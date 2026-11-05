//! macOS exclusive HAL PCM output. Unsafe is confined to typed HAL property calls and the
//! output-buffer lifetime supplied by CoreAudio. No AudioUnit, mixer or DSP. Strict mode additionally requires integer PCM.
use crate::integer::{DecodedPcm, WireFormat};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

pub struct Queued {
    pub token: u64,
    pub plan: u64,
    pub pcm: crate::streaming::Pcm,
}
pub struct Telemetry {
    pub token: AtomicU64,
    pub frame: AtomicU64,
    pub ended: AtomicBool,
    pub fault: AtomicBool,
    pub peaks: [AtomicU32; 2],
}
impl Telemetry {
    fn new(token: u64) -> Self {
        Self {
            token: AtomicU64::new(token),
            frame: AtomicU64::new(0),
            ended: AtomicBool::new(false),
            fault: AtomicBool::new(false),
            peaks: [AtomicU32::new(0), AtomicU32::new(0)],
        }
    }
}

/// The same renderer is tested with byte buffers and used by the actual HAL callback.
pub struct Renderer {
    current: Queued,
    frame: usize,
    pending: rtrb::Consumer<Queued>,
    retired: rtrb::Producer<Queued>,
    pub telemetry: Arc<Telemetry>,
    plan: Arc<AtomicU64>,
    format: WireFormat,
    empty_cycles: u32,
    exhausted: bool,
}
impl Renderer {
    fn next_frame(&mut self) -> Option<[i32; 8]> {
        let frame = loop {
            match self.current.pcm.pull(self.frame) {
                crate::streaming::Pull::Frame(frame) => break frame,
                crate::streaming::Pull::Pending => {
                    self.exhausted = false;
                    return None;
                }
                crate::streaming::Pull::Failed => {
                    self.telemetry.fault.store(true, Ordering::Relaxed);
                    return None;
                }
                crate::streaming::Pull::End => {}
            }
            if self.retired.is_full() {
                self.telemetry.fault.store(true, Ordering::Relaxed);
                return None;
            }
            match self.pending.pop() {
                Ok(next) if next.plan == self.plan.load(Ordering::Relaxed) => {
                    let old = std::mem::replace(&mut self.current, next);
                    if let Err(error) = self.retired.push(old) {
                        std::mem::forget(error);
                        self.telemetry.fault.store(true, Ordering::Relaxed);
                        return None;
                    }
                    self.frame = 0;
                    self.empty_cycles = 0;
                    self.telemetry
                        .token
                        .store(self.current.token, Ordering::Release);
                }
                Ok(stale) => {
                    if let Err(error) = self.retired.push(stale) {
                        std::mem::forget(error);
                        self.telemetry.fault.store(true, Ordering::Relaxed);
                        return None;
                    }
                }
                Err(_) => {
                    self.exhausted = true;
                    return None;
                }
            }
        };
        self.exhausted = false;
        self.frame += 1;
        self.telemetry
            .frame
            .store(self.frame as u64, Ordering::Relaxed);
        Some(frame)
    }
    pub fn render(&mut self, buffers: &mut [&mut [u8]]) {
        for buffer in buffers.iter_mut() {
            buffer.fill(0);
        }
        let expected = if self.format.planar {
            self.format.channels
        } else {
            1
        };
        if buffers.len() != expected || self.format.channels == 0 || self.format.channels > 8 {
            self.telemetry.fault.store(true, Ordering::Relaxed);
            return;
        }
        let stride = if self.format.planar {
            self.format.bytes
        } else {
            self.format.bytes * self.format.channels
        };
        if stride == 0
            || buffers
                .iter()
                .any(|b| b.len() % stride != 0 || b.len() != buffers[0].len())
        {
            self.telemetry.fault.store(true, Ordering::Relaxed);
            return;
        }
        let frames = buffers[0].len() / stride;
        let mut any = false;
        let mut peaks = [0.0f32; 2];
        for index in 0..frames {
            let Some(frame) = self.next_frame() else {
                break;
            };
            any = true;
            for channel in 0..2 {
                let raw = frame[channel.min(self.format.channels - 1)];
                let sample = if self.current.pcm.floating {
                    f64::from(f32::from_bits(raw as u32))
                } else {
                    raw as f64 / 2147483648.0
                };
                peaks[channel] = peaks[channel].max(sample.abs() as f32);
            }
            for (channel, sample) in frame.iter().copied().enumerate().take(self.format.channels) {
                let (buffer, offset) = if self.format.planar {
                    (channel, index * self.format.bytes)
                } else {
                    (0, index * stride + channel * self.format.bytes)
                };
                if let Some(target) = buffers
                    .get_mut(buffer)
                    .and_then(|b| b.get_mut(offset..offset + self.format.bytes))
                {
                    if !self
                        .format
                        .encode(sample, self.current.pcm.floating, target)
                    {
                        self.telemetry.fault.store(true, Ordering::Relaxed);
                    }
                } else {
                    self.telemetry.fault.store(true, Ordering::Relaxed);
                }
            }
        }
        for (meter, peak) in self.telemetry.peaks.iter().zip(peaks) {
            meter.store(peak.to_bits(), Ordering::Relaxed);
        }
        if !any && self.exhausted {
            self.empty_cycles = self.empty_cycles.saturating_add(frames as u32);
            if self.empty_cycles >= self.format.rate / 10 {
                self.telemetry.ended.store(true, Ordering::Release);
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use coreaudio::audio_unit::macos_helpers as helpers;
    use objc2_core_audio::*;
    use objc2_core_audio_types::*;
    use std::{
        mem::{self, MaybeUninit},
        ptr::{self, NonNull},
        time::{Duration, Instant},
    };

    // Only these plain C scalars/structs can be passed through the property helpers.
    trait Pod: Copy {}
    impl Pod for u32 {}
    impl Pod for f32 {}
    impl Pod for f64 {}
    impl Pod for AudioStreamBasicDescription {}
    impl Pod for AudioStreamRangedDescription {}
    fn address(selector: u32, scope: u32, element: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: scope,
            mElement: element,
        }
    }
    fn status(code: i32) -> Result<(), String> {
        if code == 0 {
            Ok(())
        } else {
            Err(format!("CoreAudio OSStatus {code}"))
        }
    }
    fn read<T: Pod>(object: u32, a: AudioObjectPropertyAddress) -> Result<T, String> {
        let mut value = MaybeUninit::<T>::zeroed();
        let mut bytes = mem::size_of::<T>() as u32;
        // SAFETY: T is a sealed set of POD C types, both pointers are live and bytes is
        // exactly the allocation size. HAL may not write more than ioDataSize.
        status(unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&a),
                0,
                ptr::null(),
                NonNull::from(&mut bytes),
                NonNull::from(&mut value).cast(),
            )
        })?;
        if bytes as usize != mem::size_of::<T>() {
            return Err("Dimensione proprietà HAL inattesa".into());
        }
        Ok(unsafe { value.assume_init() })
    }
    fn write<T: Pod>(object: u32, a: AudioObjectPropertyAddress, value: &T) -> Result<(), String> {
        // SAFETY: known POD property and a pointer valid for the complete call.
        status(unsafe {
            AudioObjectSetPropertyData(
                object,
                NonNull::from(&a),
                0,
                ptr::null(),
                mem::size_of::<T>() as u32,
                NonNull::from(value).cast(),
            )
        })
    }
    fn list<T: Pod>(object: u32, a: AudioObjectPropertyAddress) -> Result<Vec<T>, String> {
        let mut bytes = 0;
        status(unsafe {
            AudioObjectGetPropertyDataSize(
                object,
                NonNull::from(&a),
                0,
                ptr::null(),
                NonNull::from(&mut bytes),
            )
        })?;
        if bytes == 0 || bytes > 65536 || !(bytes as usize).is_multiple_of(mem::size_of::<T>()) {
            return Err("Elenco formati HAL non valido".into());
        }
        let count = bytes as usize / mem::size_of::<T>();
        // SAFETY: the sealed POD types accept zero initialization, and the vec owns
        // exactly ioDataSize bytes. The returned length is checked before use.
        let mut values = vec![unsafe { mem::zeroed::<T>() }; count];
        status(unsafe {
            AudioObjectGetPropertyData(
                object,
                NonNull::from(&a),
                0,
                ptr::null(),
                NonNull::from(&mut bytes),
                NonNull::new(values.as_mut_ptr())
                    .ok_or("Buffer HAL vuoto")?
                    .cast(),
            )
        })?;
        if bytes as usize > count * mem::size_of::<T>()
            || !(bytes as usize).is_multiple_of(mem::size_of::<T>())
        {
            return Err("Elenco HAL cambiato durante la lettura".into());
        }
        values.truncate(bytes as usize / mem::size_of::<T>());
        Ok(values)
    }
    fn global(selector: u32) -> AudioObjectPropertyAddress {
        address(selector, kAudioObjectPropertyScopeGlobal, 0)
    }
    fn stream_format(stream: u32, selector: u32) -> Result<AudioStreamBasicDescription, String> {
        read(stream, global(selector))
    }
    fn wire(a: &AudioStreamBasicDescription) -> Option<WireFormat> {
        let flags = a.mFormatFlags;
        if a.mFormatID != kAudioFormatLinearPCM
            || (flags & kAudioFormatFlagIsFloat == 0
                && flags & kAudioFormatFlagIsSignedInteger == 0)
            || a.mFramesPerPacket != 1
            || a.mChannelsPerFrame == 0
            || a.mChannelsPerFrame > 8
        {
            return None;
        }
        let planar = flags & kAudioFormatFlagIsNonInterleaved != 0;
        let bytes = a.mBytesPerFrame as usize
            / if planar {
                1
            } else {
                a.mChannelsPerFrame as usize
            };
        if !(1..=4).contains(&bytes)
            || a.mBitsPerChannel == 0
            || a.mBitsPerChannel > bytes as u32 * 8
            || !a.mSampleRate.is_finite()
            || a.mSampleRate <= 0.0
        {
            return None;
        }
        let floating = flags & kAudioFormatFlagIsFloat != 0;
        if floating && (bytes != 4 || a.mBitsPerChannel != 32) {
            return None;
        }
        Some(WireFormat {
            rate: a.mSampleRate as u32,
            channels: a.mChannelsPerFrame as usize,
            bits: a.mBitsPerChannel,
            bytes,
            big_endian: flags & kAudioFormatFlagIsBigEndian != 0,
            high_aligned: flags & kAudioFormatFlagIsAlignedHigh != 0,
            planar,
            floating,
        })
    }
    fn same(a: &AudioStreamBasicDescription, b: &AudioStreamBasicDescription) -> bool {
        a.mSampleRate == b.mSampleRate
            && a.mFormatID == b.mFormatID
            && a.mFormatFlags == b.mFormatFlags
            && a.mBytesPerFrame == b.mBytesPerFrame
            && a.mFramesPerPacket == b.mFramesPerPacket
            && a.mBitsPerChannel == b.mBitsPerChannel
            && a.mChannelsPerFrame == b.mChannelsPerFrame
            && a.mBytesPerPacket == b.mBytesPerPacket
    }
    fn device(name: &str, strict: bool) -> Result<(u32, u32), String> {
        let id = if name.is_empty() {
            helpers::get_default_device_id(false)
        } else {
            helpers::get_device_id_from_name(name, false)
        }
        .ok_or("DAC non trovato")?;
        let transport = helpers::get_device_transport_type(id).map_err(|e| e.to_string())?;
        if strict
            && [
                kAudioDeviceTransportTypeVirtual,
                kAudioDeviceTransportTypeAggregate,
                kAudioDeviceTransportTypeBluetooth,
                kAudioDeviceTransportTypeBluetoothLE,
                kAudioDeviceTransportTypeAirPlay,
            ]
            .contains(&transport)
        {
            return Err("Uscita virtuale, aggregata o wireless: percorso integer bit-perfect non garantibile. Scegli un DAC fisico".into());
        }
        let streams = list::<u32>(
            id,
            address(
                kAudioDevicePropertyStreams,
                kAudioObjectPropertyScopeOutput,
                0,
            ),
        )?;
        if streams.len() != 1 {
            return Err("Esclusiva: serve un dispositivo con un solo stream di uscita".into());
        }
        let stream = *streams.first().ok_or("Stream di uscita assente")?;
        Ok((id, stream))
    }
    fn candidate(
        stream: u32,
        pcm: &DecodedPcm,
        strict: bool,
    ) -> Result<AudioStreamBasicDescription, String> {
        let physical = list::<AudioStreamRangedDescription>(
            stream,
            global(kAudioStreamPropertyAvailablePhysicalFormats),
        )?;
        let virtuals = list::<AudioStreamRangedDescription>(
            stream,
            global(kAudioStreamPropertyAvailableVirtualFormats),
        )?;
        let mut options = Vec::new();
        for p in physical {
            let mut a = p.mFormat;
            a.mSampleRate = f64::from(pcm.rate);
            if f64::from(pcm.rate) < p.mSampleRateRange.mMinimum
                || f64::from(pcm.rate) > p.mSampleRateRange.mMaximum
            {
                continue;
            }
            if !wire(&a).is_some_and(|w| w.supports(pcm) && (!strict || !w.floating)) {
                continue;
            }
            if virtuals.iter().any(|v| {
                let mut b = v.mFormat;
                b.mSampleRate = a.mSampleRate;
                f64::from(pcm.rate) >= v.mSampleRateRange.mMinimum
                    && f64::from(pcm.rate) <= v.mSampleRateRange.mMaximum
                    && same(&a, &b)
            }) {
                options.push(a);
            }
        }
        options.sort_by_key(|a| {
            (
                a.mFormatFlags & kAudioFormatFlagIsFloat != 0,
                a.mBitsPerChannel,
            )
        });
        options.into_iter().next().ok_or("Il dispositivo non espone formati physical/virtual compatibili a frequenza e canali della sorgente per la modalità richiesta. Nessun fallback al mixer".into())
    }
    fn volume_unity(id: u32, channels: usize) -> Result<(), String> {
        for element in 0..=channels as u32 {
            for (selector, check) in [
                (kAudioDevicePropertyVolumeScalar, true),
                (kAudioDevicePropertyMute, false),
            ] {
                let a = address(selector, kAudioObjectPropertyScopeOutput, element);
                if unsafe { AudioObjectHasProperty(id, NonNull::from(&a)) } {
                    if check {
                        let volume = read::<f32>(id, a)?;
                        if volume != 1.0 {
                            return Err("Volume hardware non a 0 dB. Regolalo nel DAC/Configurazione MIDI Audio prima di attivare il percorso bit-perfect".into());
                        }
                    } else if read::<u32>(id, a)? != 0 {
                        return Err("Uscita hardware in mute".into());
                    }
                }
            }
        }
        Ok(())
    }
    pub fn probe(name: &str) -> Result<String, String> {
        let (id, stream) = device(name, false)?;
        let owner = helpers::get_hogging_pid(id)
            .map_err(|_| "Il dispositivo non espone l'accesso esclusivo Hog Mode")?;
        if owner != -1 {
            return Err("Il dispositivo è già occupato in esclusiva".into());
        }
        let formats = list::<AudioStreamRangedDescription>(
            stream,
            global(kAudioStreamPropertyAvailablePhysicalFormats),
        )?;
        let n = formats
            .iter()
            .filter(|f| wire(&f.mFormat).is_some_and(|w| !w.floating))
            .count();

        Ok(format!(
            "Hog Mode libero; {n} formati integer fisici. Esclusiva nativa verificata sul formato del brano; integer rigoroso richiede formati integer"
        ))
    }

    struct Lease {
        device: u32,
        stream: u32,
        old_physical: AudioStreamBasicDescription,
        old_virtual: AudioStreamBasicDescription,
        old_rate: f64,
        owned: bool,
    }
    impl Lease {
        fn acquire(
            device: u32,
            stream: u32,
            format: AudioStreamBasicDescription,
            strict: bool,
        ) -> Result<Self, String> {
            let mut lease = Self {
                device,
                stream,
                old_physical: stream_format(stream, kAudioStreamPropertyPhysicalFormat)?,
                old_virtual: stream_format(stream, kAudioStreamPropertyVirtualFormat)?,
                old_rate: read(device, global(kAudioDevicePropertyNominalSampleRate))?,
                owned: false,
            };
            if helpers::get_hogging_pid(device).map_err(|_| "Hog Mode non disponibile")? != -1 {
                return Err("DAC occupato in esclusiva da un altro processo".into());
            }
            let pid =
                helpers::toggle_hog_mode(device).map_err(|_| "Acquisizione Hog Mode rifiutata")?;
            lease.owned = pid == std::process::id() as i32;
            if !lease.owned {
                return Err("Il driver non ha concesso l'esclusiva; nessuna riproduzione condivisa alternativa".into());
            }
            if lease.old_rate != format.mSampleRate {
                write(
                    device,
                    global(kAudioDevicePropertyNominalSampleRate),
                    &format.mSampleRate,
                )?;
            }
            if !same(
                &stream_format(stream, kAudioStreamPropertyPhysicalFormat)?,
                &format,
            ) {
                write(stream, global(kAudioStreamPropertyPhysicalFormat), &format)?;
            }
            if !same(
                &stream_format(stream, kAudioStreamPropertyVirtualFormat)?,
                &format,
            ) {
                write(stream, global(kAudioStreamPropertyVirtualFormat), &format)?;
            }
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                if same(
                    &stream_format(stream, kAudioStreamPropertyPhysicalFormat)?,
                    &format,
                ) && same(
                    &stream_format(stream, kAudioStreamPropertyVirtualFormat)?,
                    &format,
                ) {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("Il DAC non ha confermato il formato integer richiesto".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            if strict {
                volume_unity(device, format.mChannelsPerFrame as usize)?;
            }
            Ok(lease)
        }
    }
    impl Drop for Lease {
        fn drop(&mut self) {
            if self.owned
                && helpers::get_hogging_pid(self.device).ok() == Some(std::process::id() as i32)
            {
                // Restore before releasing exclusivity; never release another owner's lease.
                let _ = write(
                    self.stream,
                    global(kAudioStreamPropertyPhysicalFormat),
                    &self.old_physical,
                );
                let _ = write(
                    self.stream,
                    global(kAudioStreamPropertyVirtualFormat),
                    &self.old_virtual,
                );
                let _ = write(
                    self.device,
                    global(kAudioDevicePropertyNominalSampleRate),
                    &self.old_rate,
                );
                let deadline = Instant::now() + Duration::from_secs(2);
                let mut restored = false;
                while Instant::now() < deadline {
                    restored =
                        read::<f64>(self.device, global(kAudioDevicePropertyNominalSampleRate))
                            .ok()
                            == Some(self.old_rate)
                            && stream_format(self.stream, kAudioStreamPropertyPhysicalFormat)
                                .is_ok_and(|a| same(&a, &self.old_physical))
                            && stream_format(self.stream, kAudioStreamPropertyVirtualFormat)
                                .is_ok_and(|a| same(&a, &self.old_virtual));
                    if restored {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                if !restored {
                    eprintln!(
                        "MusicCraft: il driver non ha confermato il ripristino del formato di uscita"
                    );
                }
                let _ = helpers::toggle_hog_mode(self.device);
            }
        }
    }
    struct Callback {
        renderer: Renderer,
    }
    unsafe extern "C-unwind" fn render(
        _device: u32,
        _now: *const AudioTimeStamp,
        _input: *const AudioBufferList,
        _input_time: *const AudioTimeStamp,
        output: *mut AudioBufferList,
        _output_time: *const AudioTimeStamp,
        client: *mut std::ffi::c_void,
    ) -> i32 {
        let Some(mut output) = NonNull::new(output) else {
            return 0;
        };
        if client.is_null() {
            return 0;
        }
        // SAFETY: CoreAudio keeps client and its output buffers alive until Stop and
        // Destroy complete. Only this callback mutates renderer while the proc runs.
        let callback = unsafe { &mut *client.cast::<Callback>() };
        let list = unsafe { output.as_mut() };
        let n = list.mNumberBuffers as usize;
        if n == 0 || n > 8 {
            callback
                .renderer
                .telemetry
                .fault
                .store(true, Ordering::Relaxed);
            return 0;
        }
        let raw = unsafe { std::slice::from_raw_parts_mut(list.mBuffers.as_mut_ptr(), n) };
        let mut slices: [&mut [u8]; 8] = std::array::from_fn(|_| &mut [][..]);
        for (i, b) in raw.iter_mut().enumerate() {
            let channels = if callback.renderer.format.planar {
                1
            } else {
                callback.renderer.format.channels as u32
            };
            if b.mNumberChannels != channels
                || b.mData.is_null()
                || b.mDataByteSize > 4 * 1024 * 1024
            {
                callback
                    .renderer
                    .telemetry
                    .fault
                    .store(true, Ordering::Relaxed);
                return 0;
            }
            slices[i] = unsafe {
                std::slice::from_raw_parts_mut(b.mData.cast::<u8>(), b.mDataByteSize as usize)
            };
        }
        callback.renderer.render(&mut slices[..n]);
        0
    }
    pub struct HalStream {
        lease: mem::ManuallyDrop<Lease>,
        proc: AudioDeviceIOProcID,
        callback: mem::ManuallyDrop<Box<Callback>>,
        queue: rtrb::Producer<Queued>,
        retired: rtrb::Consumer<Queued>,
        pub telemetry: Arc<Telemetry>,
        pub format: WireFormat,
        pub paused: bool,
        next_token: u64,
        strict: bool,
    }
    impl HalStream {
        pub fn open(
            name: &str,
            pcm: impl Into<crate::streaming::Pcm>,
            plan: Arc<AtomicU64>,
            strict: bool,
        ) -> Result<Self, String> {
            let pcm = pcm.into();
            let (device, stream) = device(name, strict)?;
            let asbd = candidate(stream, &pcm, strict)?;
            let fmt = wire(&asbd).ok_or("Formato integer non valido")?;
            if strict {
                volume_unity(device, pcm.channels)?;
            }
            let lease = Lease::acquire(device, stream, asbd, strict)?;
            let (queue, pending) = rtrb::RingBuffer::new(2);
            let (retired_tx, retired) = rtrb::RingBuffer::new(32);
            let telemetry = Arc::new(Telemetry::new(1));
            let renderer = Renderer {
                current: Queued {
                    token: 1,
                    plan: plan.load(Ordering::Relaxed),
                    pcm,
                },
                frame: 0,
                pending,
                retired: retired_tx,
                telemetry: telemetry.clone(),
                plan,
                format: fmt,
                empty_cycles: 0,
                exhausted: false,
            };
            let mut callback = Box::new(Callback { renderer });
            let mut proc = None;
            // SAFETY: the boxed context is stable until the proc has stopped and been
            // destroyed. Raw pointers are ABI-identical to NonNull, but allow absent
            // input/timestamp buffers: the output-only callback never dereferences them.
            status(unsafe {
                AudioDeviceCreateIOProcID(
                    device,
                    Some(mem::transmute::<
                        unsafe extern "C-unwind" fn(
                            u32,
                            *const AudioTimeStamp,
                            *const AudioBufferList,
                            *const AudioTimeStamp,
                            *mut AudioBufferList,
                            *const AudioTimeStamp,
                            *mut std::ffi::c_void,
                        ) -> i32,
                        unsafe extern "C-unwind" fn(
                            u32,
                            NonNull<AudioTimeStamp>,
                            NonNull<AudioBufferList>,
                            NonNull<AudioTimeStamp>,
                            NonNull<AudioBufferList>,
                            NonNull<AudioTimeStamp>,
                            *mut std::ffi::c_void,
                        ) -> i32,
                    >(render)),
                    (&mut *callback as *mut Callback).cast(),
                    NonNull::from(&mut proc),
                )
            })?;
            Ok(Self {
                lease: mem::ManuallyDrop::new(lease),
                proc,
                callback: mem::ManuallyDrop::new(callback),
                queue,
                retired,
                telemetry,
                format: fmt,
                paused: true,
                next_token: 2,
                strict,
            })
        }
        pub fn play(&mut self) -> Result<(), String> {
            if self.paused {
                status(unsafe { AudioDeviceStart(self.lease.device, self.proc) })?;
                self.paused = false;
            }
            Ok(())
        }
        pub fn pause(&mut self) -> Result<(), String> {
            if !self.paused {
                status(unsafe { AudioDeviceStop(self.lease.device, self.proc) })?;
                self.paused = true;
            }
            Ok(())
        }
        pub fn seek(&mut self, seconds: f64) -> Result<(), String> {
            let resume = !self.paused;
            self.pause()?;
            self.callback.renderer.current.pcm.seek(seconds);
            self.callback.renderer.frame =
                (seconds.max(0.0) * f64::from(self.format.rate)) as usize;
            self.callback.renderer.exhausted = false;
            self.telemetry
                .frame
                .store(self.callback.renderer.frame as u64, Ordering::Relaxed);
            self.telemetry.ended.store(false, Ordering::Relaxed);
            self.callback.renderer.empty_cycles = 0;
            if resume {
                self.play()?;
            }
            Ok(())
        }
        pub fn queue(
            &mut self,
            pcm: impl Into<crate::streaming::Pcm>,
            plan: u64,
        ) -> Result<u64, String> {
            let pcm = pcm.into();
            if !self.format.supports(&pcm) {
                return Err("Cambio formato: riapertura del DAC necessaria; nessun ricampionamento in esclusiva".into());
            }
            let token = self.next_token;
            self.next_token = self
                .next_token
                .checked_add(1)
                .ok_or("Contatore coda esaurito")?;
            self.queue
                .push(Queued { token, plan, pcm })
                .map_err(|_| "Coda di precaricamento piena")?;
            Ok(token)
        }
        pub fn verify(&self) -> Result<(), String> {
            if helpers::get_hogging_pid(self.lease.device).ok() != Some(std::process::id() as i32) {
                return Err("Esclusiva persa: riproduzione interrotta".into());
            }
            if read::<u32>(self.lease.device, global(kAudioDevicePropertyDeviceIsAlive))? == 0 {
                return Err("DAC scollegato".into());
            }
            let physical = stream_format(self.lease.stream, kAudioStreamPropertyPhysicalFormat)?;
            let virtual_format =
                stream_format(self.lease.stream, kAudioStreamPropertyVirtualFormat)?;
            if !same(&physical, &virtual_format) || wire(&physical) != Some(self.format) {
                return Err("Formato del DAC modificato: riproduzione interrotta".into());
            }
            if self.strict {
                volume_unity(self.lease.device, self.format.channels)
            } else {
                Ok(())
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
            let stopped = self.pause().is_ok();
            let destroyed =
                unsafe { AudioDeviceDestroyIOProcID(self.lease.device, self.proc) } == 0;
            if stopped && destroyed {
                // SAFETY: synchronous Stop and Destroy have both confirmed that HAL
                // cannot access the stable context again. Restore the device last.
                unsafe {
                    mem::ManuallyDrop::drop(&mut self.callback);
                    mem::ManuallyDrop::drop(&mut self.lease);
                }
            } else {
                // A misbehaving driver must never turn cleanup into a use-after-free.
                // Deliberately retain context/lease until process exit if quiescence
                // could not be confirmed; the OS releases process Hog Mode on exit.
                eprintln!(
                    "MusicCraft: HAL cleanup could not confirm callback shutdown; context retained"
                );
            }
        }
    }
    #[cfg(test)]
    mod native {
        use super::*;
        #[test]
        #[ignore = "claims only BlackHole loopback Hog Mode briefly and verifies restoration"]
        fn native_exclusive_hal_ownership_play_seek_and_restore() {
            let name = "BlackHole 2ch";
            let (id, stream) = device(name, false).unwrap();
            let old_rate = read::<f64>(id, global(kAudioDevicePropertyNominalSampleRate)).unwrap();
            let old_physical = stream_format(stream, kAudioStreamPropertyPhysicalFormat).unwrap();
            let old_virtual = stream_format(stream, kAudioStreamPropertyVirtualFormat).unwrap();
            assert_eq!(helpers::get_hogging_pid(id).unwrap(), -1);
            let test_rate = if old_rate == 44100.0 { 48000 } else { 44100 };
            let pcm = Arc::new(DecodedPcm {
                samples: vec![16 << 16; test_rate as usize * 2],
                floating: false,
                rate: test_rate,
                channels: 2,
                bits: 16,
            });
            {
                let mut output =
                    HalStream::open(name, pcm, Arc::new(AtomicU64::new(0)), false).unwrap();
                assert_eq!(
                    helpers::get_hogging_pid(id).unwrap(),
                    std::process::id() as i32
                );
                output.play().unwrap();
                let deadline = Instant::now() + Duration::from_secs(2);
                while output.telemetry.frame.load(Ordering::Relaxed) < 100 {
                    assert!(Instant::now() < deadline, "HAL did not render");
                    std::thread::sleep(Duration::from_millis(5));
                }
                output.pause().unwrap();
                output.seek(0.5).unwrap();
                output.verify().unwrap();
                output.play().unwrap();
                output.pause().unwrap();
                assert!(!output.telemetry.fault.load(Ordering::Relaxed));
            }
            assert_eq!(helpers::get_hogging_pid(id).unwrap(), -1);
            assert_eq!(
                read::<f64>(id, global(kAudioDevicePropertyNominalSampleRate)).unwrap(),
                old_rate
            );
            assert!(same(
                &stream_format(stream, kAudioStreamPropertyPhysicalFormat).unwrap(),
                &old_physical
            ));
            assert!(same(
                &stream_format(stream, kAudioStreamPropertyVirtualFormat).unwrap(),
                &old_virtual
            ));
        }
    }
}
#[cfg(target_os = "macos")]
pub use mac::{HalStream, probe};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn network_underflow_is_not_eof_and_does_not_advance_the_pcm_position() {
        let (stream, finish) = crate::streaming::Stream::pending_fixture();
        let (_, pending) = rtrb::RingBuffer::new(2);
        let (retired, _) = rtrb::RingBuffer::new(2);
        let telemetry = Arc::new(Telemetry::new(1));
        let mut renderer = Renderer {
            current: Queued {
                token: 1,
                plan: 0,
                pcm: stream.into(),
            },
            frame: 0,
            pending,
            retired,
            telemetry: telemetry.clone(),
            plan: Arc::new(AtomicU64::new(0)),
            format: WireFormat {
                rate: 44100,
                channels: 2,
                bits: 32,
                bytes: 4,
                big_endian: false,
                high_aligned: false,
                planar: false,
                floating: false,
            },
            empty_cycles: 0,
            exhausted: false,
        };
        let mut output = vec![9; 44100 * 8];
        renderer.render(&mut [&mut output]);
        assert!(output.iter().all(|b| *b == 0));
        assert_eq!(telemetry.frame.load(Ordering::Relaxed), 0);
        assert!(!telemetry.ended.load(Ordering::Acquire));
        assert!(!telemetry.fault.load(Ordering::Relaxed));
        finish();
        renderer.render(&mut [&mut output]);
        assert!(telemetry.ended.load(Ordering::Acquire));
    }
    fn pcm(samples: Vec<i32>) -> Arc<DecodedPcm> {
        Arc::new(DecodedPcm {
            samples,
            floating: false,
            rate: 44100,
            channels: 2,
            bits: 32,
        })
    }
    #[test]
    fn integer_gapless_is_sample_exact_across_callbacks_and_discards_stale_plans() {
        for planar in [false, true] {
            for block in [1, 3, 7, 128] {
                let a: Vec<i32> = (1..=146).map(|v| v * 1234567).collect();
                let b: Vec<i32> = (147..=364).map(|v| -(v * 1234567)).collect();
                let expected: Vec<i32> = a.iter().chain(&b).copied().collect();
                let (mut queue, pending) = rtrb::RingBuffer::new(2);
                let (retired, mut garbage) = rtrb::RingBuffer::new(32);
                queue
                    .push(Queued {
                        token: 2,
                        plan: 0,
                        pcm: pcm(vec![i32::MAX; 20]).into(),
                    })
                    .ok()
                    .unwrap();
                queue
                    .push(Queued {
                        token: 3,
                        plan: 1,
                        pcm: pcm(b).into(),
                    })
                    .ok()
                    .unwrap();
                let telemetry = Arc::new(Telemetry::new(1));
                let mut renderer = Renderer {
                    current: Queued {
                        token: 1,
                        plan: 1,
                        pcm: pcm(a).into(),
                    },
                    frame: 0,
                    pending,
                    retired,
                    telemetry: telemetry.clone(),
                    plan: Arc::new(AtomicU64::new(1)),
                    format: WireFormat {
                        rate: 44100,
                        channels: 2,
                        bits: 32,
                        bytes: 4,
                        big_endian: false,
                        high_aligned: false,
                        planar,
                        floating: false,
                    },
                    empty_cycles: 0,
                    exhausted: false,
                };
                let mut actual = Vec::new();
                while actual.len() < expected.len() {
                    let frames = block.min((expected.len() - actual.len()) / 2);
                    if planar {
                        let mut left = vec![0; frames * 4];
                        let mut right = vec![0; frames * 4];
                        renderer.render(&mut [&mut left, &mut right]);
                        for i in 0..frames {
                            actual.push(i32::from_le_bytes(
                                left[i * 4..i * 4 + 4].try_into().unwrap(),
                            ));
                            actual.push(i32::from_le_bytes(
                                right[i * 4..i * 4 + 4].try_into().unwrap(),
                            ));
                        }
                    } else {
                        let mut buffer = vec![0; frames * 8];
                        renderer.render(&mut [&mut buffer]);
                        actual.extend(
                            buffer
                                .chunks_exact(4)
                                .map(|b| i32::from_le_bytes(b.try_into().unwrap())),
                        );
                    }
                }
                assert_eq!(actual, expected, "planar={planar}, block={block}");
                assert_eq!(telemetry.token.load(Ordering::Acquire), 3);
                assert!(!telemetry.fault.load(Ordering::Relaxed));
                assert_eq!(garbage.pop().ok().unwrap().token, 2);
                assert_eq!(garbage.pop().ok().unwrap().token, 1);
            }
        }
    }
    #[test]
    fn mismatched_buffers_fail_without_writing_audio() {
        let (_, pending) = rtrb::RingBuffer::new(2);
        let (retired, _) = rtrb::RingBuffer::new(2);
        let telemetry = Arc::new(Telemetry::new(1));
        let mut renderer = Renderer {
            current: Queued {
                token: 1,
                plan: 0,
                pcm: pcm(vec![123, 456]).into(),
            },
            frame: 0,
            pending,
            retired,
            telemetry: telemetry.clone(),
            plan: Arc::new(AtomicU64::new(0)),
            format: WireFormat {
                rate: 44100,
                channels: 2,
                bits: 32,
                bytes: 4,
                big_endian: false,
                high_aligned: false,
                planar: false,
                floating: false,
            },
            empty_cycles: 0,
            exhausted: false,
        };
        let mut wrong = [255u8; 7];
        renderer.render(&mut [&mut wrong]);
        assert_eq!(wrong, [0; 7]);
        assert!(telemetry.fault.load(Ordering::Relaxed));
    }
    #[test]
    #[ignore = "read-only physical CoreAudio capability query; requires host access"]
    fn native_exclusive_capability_probe() {
        for name in crate::audio::devices() {
            println!("{name}: {}", probe(&name).unwrap_or_else(|e| e));
        }
    }
}
