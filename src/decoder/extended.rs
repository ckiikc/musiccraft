//! Extra codecs run in a local, no-network subprocess with bounded PCM queues.
//! Original remote bytes use the authenticated cache; tokens never enter process arguments.
use super::*;
use crate::integer::Transport;
use std::{
    fs::File,
    io::Read,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};
fn executable(name: &str) -> Result<PathBuf, String> {
    let name = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.into()
    };
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let parent = exe.parent().ok_or("Cannot locate codec runtime")?;
    let mut paths = vec![
        parent.join("codecs").join(&name),
        parent.join("../Resources/codecs").join(&name),
    ];
    #[cfg(any(test, debug_assertions))]
    paths.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("vendor/codecs-{}/{name}", std::env::consts::OS)),
    );
    if cfg!(target_os = "macos") {
        paths.extend([
            PathBuf::from("/opt/homebrew/bin").join(&name),
            PathBuf::from("/usr/local/bin").join(&name),
        ]);
    }
    if cfg!(target_os = "linux") {
        paths.push(PathBuf::from("/usr/bin").join(&name));
    }
    paths.into_iter().find(|p| p.is_file()).ok_or_else(|| {
        "Additional audio decoder unavailable. Reinstall the complete MusicCraft package.".into()
    })
}
fn command(name: &str) -> Result<Command, String> {
    let mut command = Command::new(executable(name)?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.stdin(Stdio::null()).stderr(Stdio::null());
    Ok(command)
}
fn probe(path: &Path, cancel: &Cancellation) -> Result<serde_json::Value, String> {
    let mut child = command("ffprobe")?.args(["-v","error","-max_alloc","67108864","-probesize","8388608","-analyzeduration","5000000","-protocol_whitelist","file,pipe","-select_streams","a:0","-show_entries","stream=codec_name,sample_rate,channels,bits_per_raw_sample,bits_per_sample,sample_fmt,duration:stream_tags:format=duration:format_tags","-of","json"]).arg(path).stdout(Stdio::piped()).spawn().map_err(|_|"Cannot start additional audio decoder")?;
    let stdout = child.stdout.take().ok_or("Missing codec metadata output")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if cancel.cancelled() || Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            return Err("Audio metadata inspection cancelled or timed out".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err("Audio metadata inspection interrupted".into());
            }
        }
    };
    let bytes = reader
        .join()
        .map_err(|_| "Audio metadata reader stopped")?
        .map_err(|_| "Cannot read audio metadata")?;
    if !status.success() || bytes.len() > 1024 * 1024 {
        return Err(
            "Unsupported or damaged audio container (DRM-protected WMA is not supported)".into(),
        );
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid codec metadata".into())
}
pub struct Decoder {
    path: PathBuf,
    info: DecodedPcm,
    pub duration: Option<f64>,
    pub source_bits: Option<u32>,
    pub gain: crate::dsp::Tags,
    child: Option<Child>,
    frames: Option<mpsc::Receiver<Result<Vec<i32>, String>>>,
    reader: Option<JoinHandle<()>>,
}
impl Decoder {
    pub fn open(
        track: &mut Track,
        root: &Path,
        auth: &AuthStore,
        cancel: Cancellation,
        strict: bool,
    ) -> Result<Self, String> {
        let path = if crate::library::is_remote(&track.source) {
            cancel.materialize(&track.source, root, auth)?
        } else {
            PathBuf::from(&track.source)
                .canonicalize()
                .map_err(|_| "Audio file unavailable")?
        };
        let metadata = probe(&path, &cancel)?;
        let stream = metadata["streams"]
            .as_array()
            .and_then(|s| s.first())
            .ok_or("File has no audio stream")?;
        let codec = stream["codec_name"].as_str().ok_or("Audio codec missing")?;
        if !matches!(
            codec,
            "ape"
                | "wavpack"
                | "wmav1"
                | "wmav2"
                | "wmapro"
                | "wmalossless"
                | "wmavoice"
                | "opus"
                | "vorbis"
                | "dsd_lsbf"
                | "dsd_msbf"
                | "dsd_lsbf_planar"
                | "dsd_msbf_planar"
                | "dst"
        ) {
            return Err("Additional decoder: unsupported audio codec".into());
        }
        let dsd = codec.starts_with("dsd_") || codec == "dst";
        let floating = dsd
            || !matches!(codec, "ape" | "wavpack" | "wmalossless")
            || stream["sample_fmt"]
                .as_str()
                .is_some_and(|s| s.starts_with("flt") || s.starts_with("dbl"));
        let mut header = [0; 32];
        let hybrid = codec == "wavpack"
            && File::open(&path)
                .and_then(|mut f| f.read_exact(&mut header))
                .is_ok()
            && &header[..4] == b"wvpk"
            && u32::from_le_bytes(header[24..28].try_into().unwrap()) & 8 != 0;
        if strict && (floating || hybrid) {
            return Err("Strict integer requires lossless integer PCM. Choose DoP for DSD, or disable Strict integer for lossy/float/hybrid audio.".into());
        }
        let parse = |key: &str| {
            stream[key]
                .as_u64()
                .or_else(|| stream[key].as_str().and_then(|s| s.parse().ok()))
        };
        let rate = if dsd {
            176400
        } else {
            parse("sample_rate").unwrap_or(0) as u32
        };
        let channels = parse("channels").unwrap_or(0) as usize;
        if !(8000..=1_536_000).contains(&rate) || !(1..=8).contains(&channels) {
            return Err("Additional decoder: unsupported rate or channel layout".into());
        }
        let bits = parse("bits_per_raw_sample")
            .filter(|b| *b > 0)
            .or_else(|| parse("bits_per_sample").filter(|b| *b > 0))
            .unwrap_or(32)
            .min(32) as u32;
        let duration = stream["duration"]
            .as_str()
            .or_else(|| metadata["format"]["duration"].as_str())
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0);
        let mut gain = crate::dsp::Tags::default();
        for tags in [&metadata["format"]["tags"], &stream["tags"]] {
            if let Some(tags) = tags.as_object() {
                for (key, value) in tags {
                    let Some(value) = value.as_str() else {
                        continue;
                    };
                    let text = value.chars().take(512).collect::<String>();
                    let n = crate::dsp::parse_number(value);
                    match key.to_ascii_lowercase().as_str() {
                        "title" if !track.metadata_loaded => track.title = text,
                        "artist" if !track.metadata_loaded => track.artist = text,
                        "album" if !track.metadata_loaded => track.album = text,
                        "album_artist" if !track.metadata_loaded => track.album_artist = text,
                        "genre" if !track.metadata_loaded => track.genre = text,
                        "date" if !track.metadata_loaded => {
                            track.year = value.get(..4).and_then(|s| s.parse().ok())
                        }
                        "track" if !track.metadata_loaded => {
                            track.number = value
                                .split('/')
                                .next()
                                .and_then(|s| s.parse().ok())
                                .unwrap_or(0)
                        }
                        "replaygain_track_gain" => gain.track_gain = n,
                        "replaygain_album_gain" => gain.album_gain = n,
                        "replaygain_track_peak" => gain.track_peak = n,
                        "replaygain_album_peak" => gain.album_peak = n,
                        _ => {}
                    }
                }
            }
        }
        if dsd {
            track.codec = "DSD → PCM".into();
        }
        let mut decoder = Self {
            path,
            info: DecodedPcm {
                transport: Transport::Pcm,
                samples: vec![],
                floating,
                rate,
                channels,
                bits: if floating { 32 } else { bits },
            },
            duration,
            source_bits: Some(if dsd { 32 } else { bits }),
            gain,
            child: None,
            frames: None,
            reader: None,
        };
        decoder.start(0.0)?;
        Ok(decoder)
    }
    pub fn info(&self) -> DecodedPcm {
        DecodedPcm {
            samples: vec![],
            ..self.info_copy()
        }
    }
    fn info_copy(&self) -> DecodedPcm {
        DecodedPcm {
            transport: Transport::Pcm,
            samples: vec![],
            floating: self.info.floating,
            rate: self.info.rate,
            channels: self.info.channels,
            bits: self.info.bits,
        }
    }
    fn start(&mut self, seconds: f64) -> Result<(), String> {
        let format = if self.info.floating { "f32le" } else { "s32le" };
        let codec = if self.info.floating {
            "pcm_f32le"
        } else {
            "pcm_s32le"
        };
        let mut command = command("ffmpeg")?;
        command
            .args([
                "-nostdin",
                "-v",
                "error",
                "-max_alloc",
                "67108864",
                "-threads",
                "1",
                "-err_detect",
                "explode",
                "-protocol_whitelist",
                "file,pipe",
                "-i",
            ])
            .arg(&self.path)
            .args([
                "-ss",
                &format!("{seconds:.9}"),
                "-map",
                "0:a:0",
                "-vn",
                "-sn",
                "-dn",
                "-ar",
                &self.info.rate.to_string(),
                "-c:a",
                codec,
                "-f",
                format,
                "pipe:1",
            ]);
        let mut child = command
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|_| "Cannot start additional audio decoder")?;
        let mut stdout = child.stdout.take().ok_or("Missing audio output")?;
        let channels = self.info.channels;
        let (tx, rx) = mpsc::sync_channel(2);
        let reader = std::thread::spawn(move || {
            let mut bytes = vec![0; 4096 * channels * 4];
            loop {
                let mut count = 0;
                while count < bytes.len() {
                    match stdout.read(&mut bytes[count..]) {
                        Ok(0) => break,
                        Ok(n) => count += n,
                        Err(_) => {
                            let _ = tx.send(Err("Additional decoder output interrupted".into()));
                            return;
                        }
                    }
                }
                if count == 0 {
                    break;
                }
                if count % (channels * 4) != 0 {
                    let _ = tx.send(Err("Incomplete PCM frame from additional decoder".into()));
                    break;
                }
                let samples = bytes[..count]
                    .chunks_exact(4)
                    .map(|b| i32::from_le_bytes(b.try_into().unwrap()))
                    .collect();
                if tx.send(Ok(samples)).is_err() {
                    break;
                }
            }
        });
        self.child = Some(child);
        self.frames = Some(rx);
        self.reader = Some(reader);
        Ok(())
    }
    pub fn next(&mut self, cancel: &impl Fn() -> bool) -> Result<Option<DecodedPcm>, String> {
        loop {
            if cancel() {
                self.stop();
                return Err("Audio preparation cancelled".into());
            }
            match self
                .frames
                .as_ref()
                .ok_or("Decoder stopped")?
                .recv_timeout(Duration::from_millis(80))
            {
                Ok(Ok(samples)) => {
                    return Ok(Some(DecodedPcm {
                        samples,
                        ..self.info_copy()
                    }));
                }
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let status = self
                        .child
                        .as_mut()
                        .ok_or("Decoder stopped")?
                        .wait()
                        .map_err(|_| "Decoder status unavailable")?;
                    if status.success() {
                        return Ok(None);
                    } else {
                        return Err(
                            "Additional decoder rejected damaged or unsupported audio".into()
                        );
                    }
                }
            }
        }
    }
    pub fn seek(&mut self, seconds: f64) -> Result<(), String> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err("Invalid audio seek position".into());
        }
        self.stop();
        self.start(seconds)
    }
    fn stop(&mut self) {
        self.frames.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
impl Drop for Decoder {
    fn drop(&mut self) {
        self.stop();
    }
}
