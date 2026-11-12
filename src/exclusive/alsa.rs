//! Direct ALSA hw PCM only: no plug/dmix/PulseAudio path may masquerade as exclusive.
use super::*;
use alsa::{
    Direction, ValueOr,
    pcm::{Access, Format, HwParams, PCM, State},
};
fn device_id(name: &str) -> Result<String, String> {
    let name = name.rsplit_once(" · ").map_or(name, |(_, driver)| driver);
    if name.starts_with("hw:") {
        return Ok(name.into());
    }
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let device = if name.is_empty() {
        host.default_output_device()
    } else {
        host.output_devices().ok().and_then(|mut devices| {
            devices.find(|d| d.description().is_ok_and(|d| d.name() == name))
        })
    }
    .ok_or("ALSA output unavailable")?;
    let description = device.description().map_err(|e| e.to_string())?;
    let driver = description
        .driver()
        .ok_or("ALSA device has no PCM identifier")?;
    if driver.starts_with("hw:") {
        Ok(driver.into())
    } else {
        Err("Exclusive ALSA requires a direct hw: device. Select a hardware output; default, plug, dmix and sound-server devices use shared playback.".into())
    }
}
pub fn probe(name: &str) -> Result<String, String> {
    let id = device_id(name)?;
    let _pcm = PCM::new(&id, Direction::Playback, true)
        .map_err(|e| format!("ALSA hardware busy or unavailable: {e}"))?;
    Ok("Direct ALSA hardware available; the track's exact sample rate and PCM format must also be accepted. Hardware bit-perfect output is not certified.".into())
}
pub struct Output {
    pcm: PCM,
    pub format: WireFormat,
    started: bool,
    strict_card: Option<String>,
}
impl Output {
    pub fn open(name: &str, pcm: &DecodedPcm, strict: bool) -> Result<Self, String> {
        let id = device_id(name)?;
        let strict_card = if strict {
            let card = id.split(',').next().unwrap().to_owned();
            unity(&card)?;
            Some(card)
        } else {
            None
        };
        let output = PCM::new(&id, Direction::Playback, true).map_err(|e| e.to_string())?;
        let mut candidates = vec![];
        if !pcm.floating {
            for (fmt, bits, bytes) in [
                (Format::S16LE, 16, 2),
                (Format::S243LE, 24, 3),
                (Format::S32LE, 32, 4),
            ] {
                if bits >= pcm.bits {
                    candidates.push((fmt, bits, bytes, false));
                }
            }
        }
        if !strict {
            candidates.push((Format::FloatLE, 32, 4, true));
        }
        for (fmt, bits, bytes, floating) in candidates {
            let result = (|| -> Result<(), alsa::Error> {
                let params = HwParams::any(&output)?;
                params.set_access(Access::RWInterleaved)?;
                params.set_rate_resample(false)?;
                params.set_channels(pcm.channels as u32)?;
                params.set_rate(pcm.rate, ValueOr::Nearest)?;
                params.set_format(fmt)?;
                params.set_buffer_time_near(60_000, ValueOr::Nearest)?;
                params.set_period_time_near(10_000, ValueOr::Nearest)?;
                output.hw_params(&params)?;
                Ok(())
            })();
            if result.is_ok() {
                return Ok(Self {
                    pcm: output,
                    format: WireFormat {
                        rate: pcm.rate,
                        channels: pcm.channels,
                        bits,
                        bytes,
                        big_endian: false,
                        high_aligned: false,
                        planar: false,
                        floating,
                    },
                    started: false,
                    strict_card,
                });
            }
        }
        Err("Direct ALSA device rejected the track's exact PCM rate/channel format".into())
    }
    pub fn verify(&self) -> Result<(), String> {
        if matches!(
            self.pcm.state(),
            State::Disconnected | State::XRun | State::Suspended
        ) {
            return Err("ALSA hardware output interrupted".into());
        }
        if let Some(card) = &self.strict_card {
            unity(card)?;
        }
        Ok(())
    }
    pub fn play(&mut self) -> Result<(), String> {
        if self.started {
            self.pcm.pause(false).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn pause(&mut self) -> Result<(), String> {
        if self.pcm.state() == State::Running {
            self.pcm
                .pause(true)
                .map_err(|e| format!("ALSA hardware pause unavailable: {e}"))?;
        }
        Ok(())
    }
    pub fn reset(&mut self, _: bool) -> Result<(), String> {
        self.pcm
            .drop()
            .and_then(|()| self.pcm.prepare())
            .map_err(|e| e.to_string())?;
        self.started = false;
        Ok(())
    }
    pub fn available(&self) -> Result<usize, String> {
        self.pcm
            .avail_update()
            .map(|n| n.max(0) as usize)
            .map_err(|e| e.to_string())
    }
    pub fn write(&mut self, bytes: &[u8]) -> Result<usize, String> {
        match self.pcm.io_bytes().writei(bytes) {
            Ok(n) => {
                self.started = true;
                Ok(n)
            }
            Err(e) if e.errno() == 11 => Ok(0),
            Err(e) => Err(format!("ALSA exclusive write interrupted: {e}")),
        }
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        let _ = self.pcm.drop();
    }
}

fn unity(card: &str) -> Result<(), String> {
    use alsa::mixer::{Mixer, Selem, SelemChannelId};
    let mixer = Mixer::new(card, false)
        .map_err(|_| "Strict integer: cannot inspect ALSA hardware mixer")?;
    let mut checked = false;
    for element in mixer.iter().filter_map(Selem::new) {
        if !element.has_playback_volume() {
            continue;
        }
        for channel in SelemChannelId::all() {
            if element.has_playback_channel(*channel) {
                let db = element
                    .get_playback_vol_db(*channel)
                    .map_err(|_| "Strict integer: hardware volume has no dB mapping")?;
                if db.0 != 0 {
                    return Err("Strict integer requires all ALSA playback controls at 0 dB; MusicCraft does not change system volume.".into());
                }
                if element.has_playback_switch()
                    && element
                        .get_playback_switch(*channel)
                        .map_err(|e| e.to_string())?
                        == 0
                {
                    return Err("Strict integer requires an unmuted ALSA hardware output".into());
                }
                checked = true;
            }
        }
    }
    if checked {
        Ok(())
    } else {
        Err("Strict integer: hardware unity volume cannot be verified on this ALSA device".into())
    }
}
