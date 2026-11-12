//! Native WASAPI exclusive PCM. All COM clients are created/used/dropped on the owned worker.
use super::*;
use wasapi::{
    AudioClient, AudioRenderClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat,
};
struct Com;
impl Com {
    fn new() -> Result<Self, String> {
        wasapi::initialize_mta().ok().map_err(|e| e.to_string())?;
        Ok(Self)
    }
}
impl Drop for Com {
    fn drop(&mut self) {
        wasapi::deinitialize();
    }
}
fn device(name: &str) -> Result<wasapi::Device, String> {
    let enumerator = DeviceEnumerator::new().map_err(|e| e.to_string())?;
    if name.is_empty() {
        enumerator.get_default_device(&Direction::Render)
    } else {
        enumerator
            .get_device_collection(&Direction::Render)
            .and_then(|devices| devices.get_device_with_name(name))
    }
    .map_err(|e| e.to_string())
}
pub fn probe(name: &str) -> Result<String, String> {
    let _com = Com::new()?;
    let device = device(name)?;
    device
        .get_iaudioclient()
        .and_then(|c| c.get_device_period())
        .map_err(|e| e.to_string())?;
    Ok("WASAPI endpoint available; exclusive access and the track's exact format are checked when playback starts. Hardware bit-perfect output is not certified.".into())
}
pub struct Output {
    render: AudioRenderClient,
    client: AudioClient,
    pub format: WireFormat,
    started: bool,
    volume: Option<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume>,
    _com: Com,
}
impl Output {
    pub fn open(name: &str, pcm: &DecodedPcm, strict: bool) -> Result<Self, String> {
        let com = Com::new()?;
        let device = device(name)?;
        let volume = if strict {
            Some(endpoint_volume(
                &device.get_id().map_err(|e| e.to_string())?,
            )?)
        } else {
            None
        };
        if let Some(volume) = &volume {
            unity(volume)?;
        }
        let mut client = device.get_iaudioclient().map_err(|e| e.to_string())?;
        let mut candidates = vec![];
        if !pcm.floating {
            for bits in [16, 24, 32] {
                if bits >= pcm.bits {
                    candidates.push((bits, SampleType::Int));
                }
            }
        }
        if !strict {
            candidates.push((32, SampleType::Float));
        }
        let format = candidates
            .into_iter()
            .find_map(|(bits, kind)| {
                let desired = WaveFormat::new(
                    bits as usize,
                    bits as usize,
                    &kind,
                    pcm.rate as usize,
                    pcm.channels,
                    None,
                );
                client.is_supported_exclusive_with_quirks(&desired).ok()
            })
            .ok_or("WASAPI rejected the track's exact PCM rate/channel format")?;
        let wire = WireFormat {
            rate: format.get_samplespersec(),
            channels: usize::from(format.get_nchannels()),
            bits: u32::from(format.get_validbitspersample()),
            bytes: usize::from(format.get_bitspersample()) / 8,
            big_endian: false,
            high_aligned: true,
            planar: false,
            floating: matches!(
                format.get_subformat().map_err(|e| e.to_string())?,
                SampleType::Float
            ),
        };
        let (default, minimum) = client.get_device_period().map_err(|e| e.to_string())?;
        let period = client
            .calculate_aligned_period_near(default.max(minimum), Some(128), &format)
            .map_err(|e| e.to_string())?;
        client
            .initialize_client(
                &format,
                &Direction::Render,
                &StreamMode::PollingExclusive {
                    period_hns: period,
                    buffer_duration_hns: (period * 4).max(600_000),
                },
            )
            .map_err(|e| format!("WASAPI exclusive access refused: {e}"))?;
        let render = client.get_audiorenderclient().map_err(|e| e.to_string())?;
        Ok(Self {
            render,
            client,
            format: wire,
            started: false,
            volume,
            _com: com,
        })
    }
    pub fn verify(&self) -> Result<(), String> {
        self.client
            .get_current_padding()
            .map_err(|e| e.to_string())?;
        if let Some(volume) = &self.volume {
            unity(volume)?;
        }
        Ok(())
    }
    pub fn play(&mut self) -> Result<(), String> {
        // Start only after the first buffer is primed.
        if self.started {
            self.client.start_stream().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn pause(&mut self) -> Result<(), String> {
        self.client.stop_stream().map_err(|e| e.to_string())
    }
    pub fn reset(&mut self, _: bool) -> Result<(), String> {
        self.client
            .stop_stream()
            .and_then(|()| self.client.reset_stream())
            .map_err(|e| e.to_string())?;
        self.started = false;
        Ok(())
    }
    pub fn available(&self) -> Result<usize, String> {
        self.client
            .get_available_space_in_frames()
            .map(|n| n as usize)
            .map_err(|e| e.to_string())
    }
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize, String> {
        let frames = bytes.len() / (self.format.channels * self.format.bytes);
        self.render
            .write_to_device(frames, bytes, None)
            .map_err(|e| e.to_string())?;
        if !self.started {
            self.client.start_stream().map_err(|e| e.to_string())?;
            self.started = true;
        }
        Ok(frames)
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        let _ = self.client.stop_stream();
    }
}

fn endpoint_volume(
    id: &str,
) -> Result<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume, String> {
    use windows::{
        Win32::{
            Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator},
            System::Com::{CLSCTX_ALL, CoCreateInstance},
        },
        core::PCWSTR,
    };
    let id: Vec<u16> = id.encode_utf16().chain(Some(0)).collect();
    // SAFETY: COM is initialized on this worker, the endpoint ID is NUL-terminated
    // and all interfaces are owned until the worker exits; no raw interface escapes.
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        enumerator
            .GetDevice(PCWSTR(id.as_ptr()))
            .and_then(|device| device.Activate(CLSCTX_ALL, None))
            .map_err(|e| e.to_string())
    }
}
fn unity(
    volume: &windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume,
) -> Result<(), String> {
    // SAFETY: valid owned endpoint interface, queried on its creating COM thread.
    let (level, muted, channels) = unsafe {
        (
            volume.GetMasterVolumeLevel(),
            volume.GetMute(),
            volume.GetChannelCount(),
        )
    };
    let all_unity = channels.is_ok_and(|count| {
        (0..count).all(|channel| {
            // SAFETY: channel indices are bounded by GetChannelCount on this interface.
            unsafe { volume.GetChannelVolumeLevel(channel) }.is_ok_and(|db| db.abs() < 0.0001)
        })
    });
    if level.is_ok_and(|db| db.abs() < 0.0001) && muted.is_ok_and(|v| !v.as_bool()) && all_unity {
        Ok(())
    } else {
        Err("Strict integer / DoP requires every hardware channel at 0 dB and unmuted; MusicCraft will not change system volume.".into())
    }
}
