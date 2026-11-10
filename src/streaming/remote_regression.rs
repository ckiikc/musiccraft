use std::sync::atomic::{AtomicBool, Ordering};
#[test]
fn playing_ranges_keep_the_verified_route_after_dns_cache_expires() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let host = "musiccraft-route-fixture.invalid";
    let source = format!("http://{host}:{port}/synthetic.wav");
    let expected: Vec<u8> = (0..BLOCK * 2 + 117).map(|n| (n % 251) as u8).collect();
    let data = expected.clone();
    let stop = Arc::new(AtomicBool::new(false));
    let done = stop.clone();
    let server = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(8);
        let mut requests = 0;
        while !done.load(Ordering::Relaxed) && std::time::Instant::now() < deadline {
            let (mut socket, _) = match listener.accept() {
                Ok(peer) => peer,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                Err(e) => panic!("{e}"),
            };
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(
                request.contains(&format!("Host: {host}:{port}\r\n")),
                "logical authority changed"
            );
            requests += 1;
            if request.starts_with("HEAD ") {
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len()
                )
                .unwrap();
            } else {
                let range = request
                    .lines()
                    .find_map(|l| l.strip_prefix("Range: bytes="))
                    .unwrap();
                let (start, end) = range.split_once('-').unwrap();
                let start: usize = start.parse().unwrap();
                let end: usize = end.parse().unwrap();
                write!(socket,"HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{}\r\nConnection: close\r\n\r\n",end-start+1,data.len()).unwrap();
                socket.write_all(&data[start..=end]).unwrap();
            }
        }
        requests
    });
    let mut easy = Easy::new();
    easy.proxy("").unwrap();
    easy.dns_cache_timeout(Duration::from_secs(1)).unwrap();
    let mut seed = curl::easy::List::new();
    seed.append(&format!("+{host}:{port}:127.0.0.1")).unwrap();
    easy.resolve(seed).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("synthetic.audio");
    let mut remote = Remote::open_with_handle(
        &source,
        temp.path(),
        target.clone(),
        &AuthStore::default(),
        Cancellation::new(Arc::new(AtomicU64::new(1)), 1, None),
        easy,
    )
    .unwrap();
    // Drop the bootstrap override and let ordinary DNS expire before another connection.
    remote
        .easy
        .get_mut()
        .unwrap()
        .resolve(curl::easy::List::new())
        .unwrap();
    std::thread::sleep(Duration::from_millis(1200));
    let mut actual = Vec::new();
    let result = remote.read_to_end(&mut actual);
    drop(remote);
    stop.store(true, Ordering::Relaxed);
    let requests = server.join().unwrap();
    assert!(
        result.is_ok(),
        "range lookup restarted after DNS expired: {result:?}"
    );
    assert_eq!(actual, expected);
    assert_eq!(requests, 4);
    assert_eq!(fs::read(target).unwrap(), expected);
}
