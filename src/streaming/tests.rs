use super::*;
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    time::Instant,
};

fn long_wav(path: &Path, seconds: u32) {
    let bytes = seconds * 48000 * 4;
    let mut header = Vec::new();
    header.extend(b"RIFF");
    header.extend((bytes + 36).to_le_bytes());
    header.extend(b"WAVEfmt ");
    header.extend(16u32.to_le_bytes());
    header.extend(1u16.to_le_bytes());
    header.extend(2u16.to_le_bytes());
    header.extend(48000u32.to_le_bytes());
    header.extend(192000u32.to_le_bytes());
    header.extend(4u16.to_le_bytes());
    header.extend(16u16.to_le_bytes());
    header.extend(b"data");
    header.extend(bytes.to_le_bytes());
    let mut file = File::create(path).unwrap();
    file.write_all(&header).unwrap();
    file.set_len(bytes as u64 + 44).unwrap();
    file.seek(SeekFrom::Start(44)).unwrap();
    for _ in 0..32 {
        file.write_all(&123i16.to_le_bytes()).unwrap();
        file.write_all(&(-123i16).to_le_bytes()).unwrap();
    }
    file.seek(SeekFrom::Start(44 + (seconds as u64 - 5) * 192000))
        .unwrap();
    for _ in 0..64 {
        file.write_all(&456i16.to_le_bytes()).unwrap();
        file.write_all(&(-456i16).to_le_bytes()).unwrap();
    }
}
fn frame(stream: &mut Stream) -> [i32; 8] {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match stream.pull() {
            Pull::Frame(f) => return f,
            Pull::Pending if Instant::now() < deadline => {
                // A seek may be draining stale frames in bounded batches.
                if stream.frames.is_empty() {
                    std::thread::sleep(Duration::from_millis(1));
                } else {
                    std::thread::yield_now();
                }
            }
            Pull::Failed => panic!("Decoder failed before buffered frame"),
            Pull::End => panic!("Unexpected end before buffered frame"),
            Pull::Pending => panic!("Timed out waiting for buffered frame"),
        }
    }
}
#[test]
fn hour_long_pcm_is_ready_with_bounded_memory_and_seek_preserves_exact_samples() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("hour.wav");
    long_wav(&path, 3600);
    let generation = Arc::new(AtomicU64::new(1));
    let plan = Arc::new(AtomicU64::new(3));
    let cancel = Cancellation::new(generation.clone(), 1, Some((plan.clone(), 3)));
    let mut track = Track::placeholder(path.to_string_lossy().into());
    let start = Instant::now();
    let mut stream = Stream::prepare(
        &mut track,
        temp.path(),
        &Default::default(),
        cancel.clone(),
        true,
        None,
    )
    .unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "must not decode the entire hour"
    );
    assert_eq!(track.duration, 3600.);
    assert!(
        stream.info.samples.is_empty(),
        "never keep the whole decoded track"
    );
    assert!(stream.frames.buffer().capacity() * std::mem::size_of::<Frame>() <= 24 * 1024 * 1024);
    assert_eq!(&frame(&mut stream)[..2], &[123 << 16, -123 << 16]);
    plan.store(4, Ordering::Relaxed);
    assert!(
        !cancel.cancelled(),
        "a new next-track plan must not cancel active audio"
    );
    stream.seek(3595.);
    assert_eq!(
        &frame(&mut stream)[..2],
        &[456 << 16, -456 << 16],
        "seek must discard old buffered samples and start at the exact requested sample"
    );
    generation.store(2, Ordering::Relaxed);
    assert!(cancel.cancelled());
}
#[test]
fn obsolete_prefetch_is_cancelled_but_a_real_started_track_survives_replanning() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("queue.wav");
    long_wav(&path, 10);
    let generation = Arc::new(AtomicU64::new(1));
    let plan = Arc::new(AtomicU64::new(3));
    let cancel = Cancellation::new(generation, 1, Some((plan.clone(), 3)));
    let mut track = Track::placeholder(path.to_string_lossy().into());
    let stream = Stream::prepare(
        &mut track,
        temp.path(),
        &Default::default(),
        cancel.clone(),
        false,
        None,
    )
    .unwrap();
    plan.store(4, Ordering::Relaxed);
    assert!(cancel.cancelled());
    drop(stream);
}
#[test]
fn progressive_lossless_and_float_packets_match_reference_decoder_and_end_once() {
    for floating in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("short.wav");
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 44100,
                bits_per_sample: if floating { 32 } else { 24 },
                sample_format: if floating {
                    hound::SampleFormat::Float
                } else {
                    hound::SampleFormat::Int
                },
            },
        )
        .unwrap();
        for i in 0..4102 {
            if floating {
                writer.write_sample((i as f32 - 2051.) / 4000.).unwrap();
            } else {
                writer.write_sample((i - 2051) * 123).unwrap();
            }
        }
        writer.finalize().unwrap();
        let reference = DecodedPcm::read_mode(&path, || false, !floating).unwrap();
        let mut track = Track::placeholder(path.to_string_lossy().into());
        let mut stream = Stream::prepare(
            &mut track,
            temp.path(),
            &Default::default(),
            Cancellation::new(Arc::new(AtomicU64::new(0)), 0, None),
            !floating,
            None,
        )
        .unwrap();
        let mut result = vec![];
        loop {
            match stream.pull() {
                Pull::Frame(f) => result.extend_from_slice(&f[..2]),
                Pull::Pending => std::thread::yield_now(),
                Pull::End => break,
                Pull::Failed => panic!("decoder failed"),
            }
        }
        assert_eq!(reference.samples, result);
        assert!(matches!(stream.pull(), Pull::End));
    }
}

#[test]
#[ignore = "explicit authenticated synthetic FTP fixture, no personal server"]
fn live_ftp_progressive_hour_file_starts_seeks_and_prefetches_without_full_download() {
    let root = std::env::var("AUREA_TEST_FTP_URL").unwrap();
    let folder = std::path::PathBuf::from(std::env::var_os("AUREA_TEST_FTP_ROOT").unwrap());
    let path = folder.join("Hour [gapless].wav");
    long_wav(&path, 3600);
    let url = crate::network::child_url(&root, "Hour [gapless].wav", false).unwrap();
    let mut source = crate::network::FtpSource::smartphone();
    source.set_url(&root).unwrap();
    source.username = "fixture".into();
    source.password = "fixture-only".into();
    let auth = AuthStore::default();
    auth.insert(&source).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let generation = Arc::new(AtomicU64::new(8));
    let plan = Arc::new(AtomicU64::new(4));
    let cancel = Cancellation::new(generation.clone(), 8, None);
    let mut track = Track::placeholder(url.clone());
    let start = Instant::now();
    let mut stream =
        Stream::prepare(&mut track, temp.path(), &auth, cancel.clone(), true, None).unwrap();
    let first_ms = start.elapsed().as_millis();
    assert!(first_ms < 5000);
    assert!(cancel.transferred.load(Ordering::Relaxed) < 4 * 1024 * 1024);
    let ahead = stream.frames.buffer().capacity();
    assert!(ahead >= 44100 * 20);
    assert!(ahead * std::mem::size_of::<Frame>() <= 64 * 1024 * 1024);
    assert_eq!(&frame(&mut stream)[..2], &[123 << 16, -123 << 16]);
    // Pull at the real sample rate while the FTP server stalls for eight seconds.
    let play = Instant::now();
    let frames_per_tick = stream.info.rate / 100;
    for tick in 0..2_000u64 {
        std::thread::sleep(
            (play + Duration::from_millis(tick * 10)).saturating_duration_since(Instant::now()),
        );
        for _ in 0..frames_per_tick {
            assert!(
                matches!(stream.pull(), Pull::Frame(_)),
                "PCM underrun at tick {tick}"
            );
        }
    }
    stream.seek(3595.);
    assert_eq!(&frame(&mut stream)[..2], &[456 << 16, -456 << 16]);
    let (tx, rx) = mpsc::channel();
    let start = Instant::now();
    crate::jobs::prefetch(
        Track::placeholder(url),
        temp.path().join("prefetch"),
        auth,
        generation,
        8,
        tx,
        crate::jobs::PrepareOptions {
            next_plan: Some((plan, 4)),
            exclusive: Some(true),
            ..Default::default()
        },
    );
    let ready = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        matches!(ready, crate::jobs::Job::Ready(_, Some(_), 8, Some(4))),
        "must queue the head, not materialize and decode the hour"
    );
    println!(
        "progressive FTP: 3600 seconds, first PCM {first_ms} ms, next-track ready {} ms, bounded FTP PCM <=64 MiB",
        start.elapsed().as_millis()
    );
    let leading = folder.join("    Leading.flac");
    std::fs::copy(folder.join("Album Città/02 Interlude.flac"), &leading).unwrap();
    let mut track =
        Track::placeholder(crate::network::child_url(&root, "    Leading.flac", false).unwrap());
    let auth = AuthStore::default();
    auth.insert(&source).unwrap();
    let mut leading_stream = Stream::prepare(
        &mut track,
        temp.path(),
        &auth,
        Cancellation::new(Arc::new(AtomicU64::new(0)), 0, None),
        true,
        None,
    )
    .unwrap();
    assert_eq!(track.title, "Interlude");
    assert_eq!(track.artist, "Aurea Test Ensemble");
    let gain = leading_stream.replay_gain(&track.source, temp.path(), &auth);
    assert_eq!(gain.track_gain, Some(-6.));
    assert!((0..100).any(|_| frame(&mut leading_stream)[0] != 0));
    drop(leading_stream);
    std::fs::remove_file(leading).unwrap();
    std::fs::remove_file(path).unwrap();
}
