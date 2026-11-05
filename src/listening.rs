//! On-demand public metadata. No file paths, FTP addresses or credentials leave the player.
use crate::library::Track;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct Query {
    pub artist: String,
    pub title: String,
    pub album: String,
    pub duration: u32,
}
impl Query {
    pub fn from_track(t: &Track) -> Option<Self> {
        let title = lookup_title(&t.title);
        let mut artist = t.artist.trim();
        let mut title = title.as_str();
        if ["", "unknownartist", "artistasconosciuto"].contains(&canonical(artist).as_str()) {
            let split = [" - ", " – ", " — "]
                .into_iter()
                .filter_map(|separator| title.find(separator).map(|i| (i, separator)))
                .min_by_key(|(i, _)| *i);
            let (i, separator) = split?;
            let (a, b) = (title[..i].trim(), title[i + separator.len()..].trim());
            if !(2..=120).contains(&a.chars().count())
                || b.is_empty()
                || a.contains("://")
                || a.chars().all(|c| c.is_ascii_digit())
            {
                return None;
            }
            artist = a;
            title = b;
        }
        Some(Self {
            artist: artist.chars().take(256).collect(),
            title: title.chars().take(256).collect(),
            album: t.album.trim().chars().take(256).collect(),
            duration: if t.duration.is_finite() {
                t.duration.round().clamp(0.0, 86400.0) as u32
            } else {
                0
            },
        })
    }
}
#[derive(Clone, Debug)]
pub struct Biography {
    pub title: String,
    pub text: String,
    pub url: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub time: f64,
    pub text: String,
}
#[derive(Clone, Debug)]
pub struct Lyrics {
    pub lines: Vec<Line>,
    pub plain: String,
    pub instrumental: bool,
    pub url: String,
}
impl Lyrics {
    pub fn active(&self, position: f64) -> Option<usize> {
        if !position.is_finite() || position < 0.0 {
            return None;
        }
        self.lines
            .partition_point(|l| l.time <= position)
            .checked_sub(1)
    }
}
#[derive(Clone, Debug, Default)]
pub enum State<T> {
    #[default]
    Empty,
    Loading,
    Ready(T),
    Missing,
    Error(String),
}
type Answer<T> = Result<Option<T>, String>;
#[derive(Debug, Clone)]
enum FetchError {
    Cancelled,
    RateLimited,
    Temporary(String),
    Unavailable(String),
}
impl From<String> for FetchError {
    fn from(s: String) -> Self {
        Self::Unavailable(s)
    }
}
impl From<&str> for FetchError {
    fn from(s: &str) -> Self {
        Self::Unavailable(s.into())
    }
}
impl From<FetchError> for String {
    fn from(error: FetchError) -> Self {
        match error {
            FetchError::Cancelled => "Ricerca annullata".into(),
            FetchError::RateLimited => "Servizio occupato. Riprova più tardi.".into(),
            FetchError::Temporary(s) | FetchError::Unavailable(s) => s,
        }
    }
}
type FetchAnswer = Result<Option<Value>, FetchError>;
type Cache<K, V> = HashMap<K, (Answer<Arc<V>>, Instant)>;
enum Payload {
    Artist(Answer<Biography>),
    Lyrics(Answer<Lyrics>),
}
struct Response {
    epoch: u64,
    query: Query,
    payload: Payload,
}
struct Request {
    epoch: u64,
    language: String,
    query: Query,
    artist: bool,
    lyrics: bool,
}
pub struct Info {
    tx: mpsc::Sender<Request>,
    rx: mpsc::Receiver<Response>,
    generation: Arc<AtomicU64>,
    query: Option<Query>,
    language: String,
    artists: Cache<String, Biography>,
    songs: Cache<Query, Lyrics>,
    pub artist: State<Arc<Biography>>,
    pub lyrics: State<Arc<Lyrics>>,
}
impl Default for Info {
    fn default() -> Self {
        let (tx, requests) = mpsc::channel::<Request>();
        let (responses, rx) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let epoch = generation.clone();
        std::thread::spawn(move || {
            while let Ok(mut r) = requests.recv() {
                // Only the most recent visible track can schedule a request.
                while let Ok(newer) = requests.try_recv() {
                    r = newer;
                }
                if epoch.load(Ordering::Relaxed) != r.epoch {
                    continue;
                }
                let cancelled = || epoch.load(Ordering::Relaxed) != r.epoch;
                // The two services are independent: a slow biography must not delay lyrics.
                // The dispatcher waits for this bounded pair before accepting the next query.
                std::thread::scope(|scope| {
                    if r.artist {
                        scope.spawn(|| {
                            let payload = Payload::Artist(fetch_artist_localized(
                                &r.query.artist,
                                &r.language,
                                &mut |url| get(url, &cancelled),
                            ));
                            let _ = responses.send(Response {
                                epoch: r.epoch,
                                query: r.query.clone(),
                                payload,
                            });
                        });
                    }
                    if r.lyrics {
                        scope.spawn(|| {
                            let payload = Payload::Lyrics(fetch_lyrics(&r.query, &cancelled));
                            let _ = responses.send(Response {
                                epoch: r.epoch,
                                query: r.query.clone(),
                                payload,
                            });
                        });
                    }
                });
            }
        });
        Self {
            tx,
            rx,
            generation,
            query: None,
            language: String::new(),
            artists: HashMap::new(),
            songs: HashMap::new(),
            artist: State::Empty,
            lyrics: State::Empty,
        }
    }
}
fn cached<K: Eq + std::hash::Hash, V>(cache: &Cache<K, V>, key: &K) -> Option<Answer<Arc<V>>> {
    let (answer, at) = cache.get(key)?;
    let ttl = if matches!(answer, Ok(Some(_))) {
        24 * 3600
    } else {
        5 * 60
    };
    (at.elapsed() < Duration::from_secs(ttl)).then(|| answer.clone())
}
fn evict_oldest<K: Clone + Eq + std::hash::Hash, V>(cache: &mut Cache<K, V>) {
    if cache.len() >= 128
        && let Some(key) = cache
            .iter()
            .min_by_key(|(_, (_, at))| *at)
            .map(|(key, _)| key.clone())
    {
        cache.remove(&key);
    }
}
fn state<T>(answer: Answer<T>) -> State<T> {
    match answer {
        Ok(Some(v)) => State::Ready(v),
        Ok(None) => State::Missing,
        Err(e) => State::Error(e),
    }
}
impl Info {
    pub fn retry(&mut self) {
        if let Some(query) = self.query.take() {
            if !matches!(self.artist, State::Ready(_)) {
                self.artists
                    .remove(&artist_cache_key(&self.language, &query.artist));
            }
            if !matches!(self.lyrics, State::Ready(_)) {
                self.songs.remove(&query);
            }
            self.update(true, Some(query));
        }
    }
    pub fn update(&mut self, open: bool, query: Option<Query>) -> bool {
        let query = if open { query } else { None };
        let language = crate::i18n::language();
        let changed = query != self.query || self.language != language;
        self.language = language.into();
        if changed {
            let epoch = self
                .generation
                .fetch_add(1, Ordering::Relaxed)
                .wrapping_add(1);
            self.query = query.clone();
            self.artist = State::Empty;
            self.lyrics = State::Empty;
            if let Some(q) = query {
                let artist = cached(&self.artists, &artist_cache_key(&self.language, &q.artist));
                let lyrics = cached(&self.songs, &q);
                let lyrics_needed = !q.title.is_empty();
                let request = Request {
                    epoch,
                    language: self.language.clone(),
                    query: q,
                    artist: artist.is_none(),
                    lyrics: lyrics_needed && lyrics.is_none(),
                };
                self.artist = artist.map_or(State::Loading, state);
                self.lyrics = lyrics.map_or(
                    if lyrics_needed {
                        State::Loading
                    } else {
                        State::Missing
                    },
                    state,
                );
                if request.artist || request.lyrics {
                    let _ = self.tx.send(request);
                }
            }
        }
        while let Ok(response) = self.rx.try_recv() {
            self.apply(response);
        }
        changed
    }
    fn apply(&mut self, r: Response) {
        if r.epoch != self.generation.load(Ordering::Relaxed)
            || self.query.as_ref() != Some(&r.query)
        {
            return;
        }
        match r.payload {
            Payload::Artist(answer) => {
                let answer = answer.map(|v| v.map(Arc::new));
                evict_oldest(&mut self.artists);
                self.artists.insert(
                    artist_cache_key(&self.language, &r.query.artist),
                    (answer.clone(), Instant::now()),
                );
                self.artist = state(answer);
            }
            Payload::Lyrics(answer) => {
                let answer = answer.map(|v| v.map(Arc::new));
                evict_oldest(&mut self.songs);
                self.songs.insert(r.query, (answer.clone(), Instant::now()));
                self.lyrics = state(answer);
            }
        }
    }
    #[cfg(any(debug_assertions, test))]
    pub fn fixture(&mut self, query: Query, bio: Biography, lyrics: Lyrics) {
        self.language = crate::i18n::language().into();
        self.generation.fetch_add(1, Ordering::Relaxed);
        let bio = Arc::new(bio);
        let lyrics = Arc::new(lyrics);
        self.artists.insert(
            artist_cache_key(&self.language, &query.artist),
            (Ok(Some(bio.clone())), Instant::now()),
        );
        self.songs
            .insert(query.clone(), (Ok(Some(lyrics.clone())), Instant::now()));
        self.query = Some(query);
        self.artist = State::Ready(bio);
        self.lyrics = State::Ready(lyrics);
    }
}
impl Drop for Info {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
}

pub(crate) fn canonical(s: &str) -> String {
    s.nfkd()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
fn artist_article(s: &str) -> Option<&'static str> {
    ["The", "Il", "Lo", "La", "I", "Gli", "Le", "L'", "L’"]
        .into_iter()
        .find(|article| s.eq_ignore_ascii_case(article))
}
fn artist_base(s: &str) -> &str {
    let s = s.trim();
    let s = s
        .rsplit_once(',')
        .filter(|(base, suffix)| !base.trim().is_empty() && artist_article(suffix.trim()).is_some())
        .map_or(s, |(base, _)| base.trim());
    let rest = s
        .split_once(char::is_whitespace)
        .filter(|(prefix, _)| artist_article(prefix).is_some())
        .map(|(_, rest)| rest.trim())
        .or_else(|| {
            ["L'", "l'", "L’", "l’"]
                .into_iter()
                .find_map(|prefix| s.strip_prefix(prefix).map(str::trim))
        });
    rest.filter(|rest| rest.chars().count() >= 3 && artist_article(rest).is_none())
        .unwrap_or(s)
}
pub(crate) fn artist_key(s: &str) -> String {
    canonical(artist_base(s))
}
fn lookup_artist(artist: &str) -> String {
    artist
        .trim()
        .rsplit_once(',')
        .filter(|(base, suffix)| !base.trim().is_empty() && artist_article(suffix.trim()).is_some())
        .map_or_else(
            || artist.trim().to_owned(),
            |(base, suffix)| {
                let article = artist_article(suffix.trim()).unwrap();
                format!(
                    "{article}{}{}",
                    if article.ends_with(['\'', '’']) {
                        ""
                    } else {
                        " "
                    },
                    base.trim()
                )
            },
        )
        .nfc()
        .collect()
}
pub(crate) fn lookup_title(title: &str) -> String {
    let mut title = title.trim();
    let digits = title.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=3).contains(&digits) {
        let tail = title[digits..].trim_start();
        if let Some(tail) = tail
            .strip_prefix(". ")
            .or_else(|| tail.strip_prefix("- "))
            .or_else(|| {
                (digits >= 2 && title[digits..].starts_with(char::is_whitespace)).then_some(tail)
            })
            && !tail.trim().is_empty()
        {
            title = tail.trim();
        }
    }
    let edition = |s: &str| {
        let lower = s.to_lowercase();
        (lower.contains("remaster")
            || [
                "official audio",
                "official video",
                "official music video",
                "lyric video",
                "lyrics video",
            ]
            .contains(&lower.as_str()))
            && lower
                .split(|c: char| !c.is_alphanumeric())
                .filter(|s| !s.is_empty())
                .all(|s| {
                    s.chars().all(|c| c.is_ascii_digit())
                        || [
                            "remaster",
                            "remastered",
                            "remasters",
                            "digital",
                            "version",
                            "stereo",
                            "mono",
                            "official",
                            "music",
                            "audio",
                            "video",
                            "lyric",
                            "lyrics",
                        ]
                        .contains(&s)
                })
    };
    for (open, close) in [('(', ')'), ('[', ']')] {
        if title.ends_with(close)
            && let Some((head, suffix)) = title.rsplit_once(open)
            && edition(suffix.trim_end_matches(close))
        {
            title = head.trim();
        }
    }
    if let Some((head, suffix)) = title.rsplit_once(" - ")
        && edition(suffix)
    {
        title = head.trim();
    }
    title.nfc().collect()
}
fn timestamp(s: &str) -> Option<f64> {
    let (m, s) = s.split_once(':')?;
    if m.is_empty() || m.len() > 3 || !m.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let seconds = s.parse::<f64>().ok()?;
    if !seconds.is_finite() || !(0.0..60.0).contains(&seconds) {
        return None;
    }
    Some(m.parse::<u32>().ok()? as f64 * 60.0 + seconds)
}
pub fn parse_lrc(raw: &str) -> Vec<Line> {
    let offset = raw
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("[offset:")
                .and_then(|s| s.strip_suffix(']'))
                .and_then(|s| s.trim().parse::<i32>().ok())
        })
        .unwrap_or(0) as f64
        / 1000.0;
    let mut result = Vec::new();
    for original in raw.lines().take(4000) {
        if result.len() >= 4000 {
            break;
        }
        let mut text = original.trim_start_matches('\u{feff}').trim();
        let mut times = Vec::new();
        while let Some(s) = text.strip_prefix('[') {
            let Some((stamp, rest)) = s.split_once(']') else {
                break;
            };
            if let Some(time) = timestamp(stamp)
                && times.len() < 32
            {
                times.push((time + offset).max(0.0));
            }
            text = rest.trim_start();
        }
        // Enhanced LRC word stamps are removed; the UI explicitly follows lines.
        let mut clean = String::new();
        while !text.is_empty() && clean.len() < 8000 {
            if let Some(s) = text.strip_prefix('<')
                && let Some((stamp, rest)) = s.split_once('>')
                && timestamp(stamp).is_some()
            {
                text = rest;
                continue;
            }
            let c = text.chars().next().unwrap();
            clean.push(c);
            text = &text[c.len_utf8()..];
        }
        let clean: String = clean.trim().chars().take(2000).collect();
        for time in times.into_iter().take(4000 - result.len()) {
            result.push(Line {
                time,
                text: clean.clone(),
            });
        }
    }
    result.sort_by(|a, b| a.time.total_cmp(&b.time));
    result.dedup();
    result.truncate(4000);
    result
}

fn get(url: &url::Url, cancelled: &dyn Fn() -> bool) -> FetchAnswer {
    retry_temporary(&mut || get_once(url, cancelled), cancelled)
}
fn retry_temporary(
    fetch: &mut impl FnMut() -> FetchAnswer,
    cancelled: &dyn Fn() -> bool,
) -> FetchAnswer {
    let result = fetch();
    if matches!(result, Err(FetchError::Temporary(_))) {
        if cancelled() {
            return Err(FetchError::Cancelled);
        }
        // get_once applies the same per-host throttle and 429 backoff to this retry.
        return fetch().map_err(|e| match e {
            FetchError::Temporary(s) => FetchError::Unavailable(s),
            e => e,
        });
    }
    result
}
fn get_once(url: &url::Url, cancelled: &dyn Fn() -> bool) -> FetchAnswer {
    #[derive(Default)]
    struct Throttle {
        last: Option<Instant>,
        blocked: Option<Instant>,
    }
    static THROTTLE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, Throttle>>> =
        std::sync::OnceLock::new();
    let throttle = THROTTLE.get_or_init(Default::default);
    let host = url.host_str().unwrap_or_default().to_string();
    let interval = if host == "lrclib.net" {
        Duration::from_secs(1)
    } else {
        Duration::from_millis(350)
    };
    loop {
        if cancelled() {
            return Err(FetchError::Cancelled);
        }
        let wait = {
            let mut hosts = throttle.lock().map_err(|_| "Servizio non disponibile")?;
            let state = hosts.entry(host.clone()).or_default();
            if state.blocked.is_some_and(|until| Instant::now() < until) {
                return Err(FetchError::RateLimited);
            }
            let wait = state
                .last
                .map_or(Duration::ZERO, |at| interval.saturating_sub(at.elapsed()));
            if wait.is_zero() {
                state.last = Some(Instant::now());
            }
            wait
        };
        if wait.is_zero() {
            break;
        }
        std::thread::sleep(wait.min(Duration::from_millis(50)));
    }
    let mut easy = curl::easy::Easy::new();
    let setup = (|| -> Result<(), curl::Error> {
        easy.url(url.as_str())?;
        easy.useragent(concat!(
            "MusicCraft/",
            env!("CARGO_PKG_VERSION"),
            " (macOS; community source preview)"
        ))?;
        easy.connect_timeout(Duration::from_secs(4))?;
        easy.timeout(Duration::from_secs(20))?;
        easy.progress(true)?;
        Ok(())
    })();
    setup.map_err(|_| "Richiesta non disponibile".to_string())?;
    let mut bytes = Vec::new();
    let mut retry_after = 300;
    {
        let mut transfer = easy.transfer();
        transfer
            .header_function(|header| {
                if let Ok(header) = std::str::from_utf8(header)
                    && let Some((name, value)) = header.split_once(':')
                    && name.eq_ignore_ascii_case("Retry-After")
                {
                    retry_after = retry_delay(value.trim()).as_secs();
                }
                true
            })
            .map_err(|_| "Richiesta non disponibile")?;
        transfer
            .write_function(|data| {
                if cancelled() || bytes.len() + data.len() > 2 * 1024 * 1024 {
                    return Ok(0);
                }
                bytes.extend_from_slice(data);
                Ok(data.len())
            })
            .map_err(|_| "Richiesta non disponibile")?;
        transfer
            .progress_function(|_, _, _, _| !cancelled())
            .map_err(|_| "Richiesta non disponibile")?;
        transfer.perform().map_err(|e| {
            if cancelled() {
                FetchError::Cancelled
            } else if e.is_operation_timedout()
                || e.is_couldnt_connect()
                || e.is_recv_error()
                || e.is_send_error()
            {
                FetchError::Temporary("Connessione non disponibile o tempo scaduto".into())
            } else {
                FetchError::Unavailable("Connessione non disponibile o tempo scaduto".into())
            }
        })?;
    }
    match easy.response_code().unwrap_or(0) {
        200 => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| "Risposta del servizio non valida".into()),
        404 => Ok(None),
        408 | 500 | 502 | 503 | 504 => Err(FetchError::Temporary(
            "Il servizio non è disponibile. Riapri la scheda più tardi.".into(),
        )),
        429 => {
            if let Ok(mut hosts) = throttle.lock() {
                hosts.entry(host).or_default().blocked = Instant::now()
                    .checked_add(Duration::from_secs(retry_after))
                    .or_else(|| Instant::now().checked_add(Duration::from_secs(365 * 86400)));
            }
            Err(FetchError::RateLimited)
        }
        _ => Err("Il servizio non è disponibile. Riapri la scheda più tardi.".into()),
    }
}
fn retry_delay(value: &str) -> Duration {
    Duration::from_secs(
        value
            .parse::<u64>()
            .ok()
            .or_else(|| {
                httpdate::parse_http_date(value)
                    .ok()
                    .and_then(|date| date.duration_since(std::time::SystemTime::now()).ok())
                    .map(|delay| delay.as_secs() + 1)
            })
            .unwrap_or(300)
            .max(300),
    )
}
fn wiki_candidate<'a>(pages: &'a [Value], artist: &str) -> Option<&'a Value> {
    let requested = canonical(artist);
    let artist = artist_key(artist);
    if artist.is_empty() {
        return None;
    }
    pages
        .iter()
        .filter_map(|p| {
            if p.get("missing").is_some() || p.pointer("/pageprops/disambiguation").is_some() {
                return None;
            }
            let title = p["title"].as_str()?;
            let text = p["extract"].as_str()?;
            let identity = title.split_once(" (").map_or(title, |(base, _)| base);
            if artist_key(identity) != artist || text.trim().is_empty() {
                return None;
            }
            if let Some((_, qualifier)) = title.split_once(" (") {
                let qualifier = canonical(qualifier);
                if ![
                    "gruppomusicale",
                    "band",
                    "cantante",
                    "cantautore",
                    "cantautrice",
                    "musicista",
                    "rapper",
                    "compositore",
                    "compositrice",
                    "discjockey",
                    "orchestra",
                    "duomusicale",
                    "ensemble",
                    "musician",
                    "singer",
                    "songwriter",
                    "composer",
                    "duo",
                    "musicalgroup",
                ]
                .iter()
                .any(|q| qualifier.starts_with(q))
                {
                    return None;
                }
            }
            let lower = text.to_lowercase();
            if ![
                "musical",
                "musicista",
                "cantant",
                "cantautor",
                "cantautr",
                "compositor",
                "compositr",
                "rapper",
                "band",
                "disc jockey",
                "chitarrist",
                "pianist",
                "orchestra",
                "sassofonist",
                "violinist",
                "bassista",
                "batterista",
                "flautista",
                "gruppo rock",
                "gruppo pop",
                "gruppo jazz",
                "gruppo folk",
                "gruppo punk",
                "gruppo metal",
                "gruppo hip hop",
                "gruppo vocale",
                "gruppo corale",
                "singer",
                "songwriter",
                "composer",
                "musician",
                "music group",
            ]
            .iter()
            .any(|s| lower.contains(s))
            {
                return None;
            }
            Some((
                (
                    canonical(identity) == requested,
                    artist_key(title) == artist,
                ),
                p,
            ))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, p)| p)
}
fn artist_cache_key(language: &str, artist: &str) -> String {
    format!("{language}:{}", canonical(&lookup_artist(artist)))
}
fn fetch_artist_localized(
    artist: &str,
    locale: &str,
    fetch: &mut impl FnMut(&url::Url) -> FetchAnswer,
) -> Answer<Biography> {
    let locale = crate::i18n::normalize(locale);
    let preferred = match locale {
        "pt-br" => "pt",
        "zh-hans" | "zh-hant" => "zh",
        value => value,
    };
    if preferred == "it" {
        if let Some(bio) = fetch_artist_using(artist, fetch)? {
            return Ok(Some(bio));
        }
        return fetch_artist_using_in(artist, "en", fetch);
    }
    // Validate the artist in a language whose musical qualifiers we understand,
    // then follow Wikipedia's interlanguage identity rather than guessing a
    // translated/transliterated name or accepting an unrelated search result.
    let english = fetch_artist_using_in(artist, "en", fetch)?;
    if preferred == "en" && english.is_some() {
        return Ok(english);
    }
    let anchor = match english.as_ref() {
        Some(bio) => bio.clone(),
        None => match fetch_artist_using(artist, fetch)? {
            Some(bio) => bio,
            None => return Ok(None),
        },
    };
    let source = url::Url::parse(&anchor.url)
        .ok()
        .and_then(|u| {
            u.host_str()
                .map(|h| h.split('.').next().unwrap_or("en").to_string())
        })
        .unwrap_or_else(|| "en".into());
    let mut link = url::Url::parse(&format!("https://{source}.wikipedia.org/w/api.php")).unwrap();
    link.query_pairs_mut().extend_pairs([
        ("action", "query"),
        ("prop", "langlinks"),
        ("titles", anchor.title.as_str()),
        ("lllang", preferred),
        ("lllimit", "1"),
        ("format", "json"),
        ("formatversion", "2"),
    ]);
    let linked = match fetch(&link) {
        Ok(Some(value)) => value
            .pointer("/query/pages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|p| p.get("langlinks").and_then(Value::as_array))
            .flatten()
            .find(|v| v["lang"] == preferred)
            .and_then(|v| v.get("title").or_else(|| v.get("*")))
            .and_then(Value::as_str)
            .map(str::to_string),
        Err(FetchError::Cancelled | FetchError::RateLimited) => {
            return Err("Ricerca annullata o servizio occupato".into());
        }
        _ => None,
    };
    if let Some(title) = linked {
        let mut url =
            url::Url::parse(&format!("https://{preferred}.wikipedia.org/w/api.php")).unwrap();
        url.query_pairs_mut().extend_pairs([
            ("action", "query"),
            ("prop", "extracts|pageprops"),
            ("titles", title.as_str()),
            ("explaintext", "1"),
            ("exchars", "5000"),
            ("ppprop", "disambiguation"),
            ("format", "json"),
            ("formatversion", "2"),
        ]);
        if preferred == "zh" {
            url.query_pairs_mut().append_pair("variant", locale);
        }
        match fetch(&url) {
            Ok(Some(json)) => {
                if let Some(page) = json
                    .pointer("/query/pages")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .find(|p| {
                        p.get("missing").is_none()
                            && p.pointer("/pageprops/disambiguation").is_none()
                            && p["title"]
                                .as_str()
                                .is_some_and(|t| canonical(t) == canonical(&title))
                    })
                    && let Some(text) = page["extract"].as_str().filter(|s| !s.trim().is_empty())
                {
                    let mut source_url =
                        url::Url::parse(&format!("https://{preferred}.wikipedia.org/wiki/"))
                            .unwrap();
                    source_url
                        .path_segments_mut()
                        .unwrap()
                        .pop_if_empty()
                        .push(&title.replace(' ', "_"));
                    if preferred == "zh" {
                        source_url.query_pairs_mut().append_pair("variant", locale);
                    }
                    return Ok(Some(Biography {
                        title,
                        text: text.chars().take(8000).collect(),
                        url: source_url.into(),
                    }));
                }
            }
            Err(FetchError::Cancelled | FetchError::RateLimited) => {
                return Err("Ricerca annullata o servizio occupato".into());
            }
            _ => {}
        }
    }
    Ok(english)
}
#[cfg(test)]
fn wiki_biography(json: &Value, artist: &str) -> Option<Biography> {
    wiki_biography_in(json, artist, "it")
}
fn wiki_biography_in(json: &Value, artist: &str, language: &str) -> Option<Biography> {
    let mut resolved = artist.to_string();
    if let Some(redirects) = json.pointer("/query/redirects").and_then(Value::as_array) {
        for redirect in redirects.iter().take(8) {
            if redirect["from"]
                .as_str()
                .is_some_and(|s| canonical(s) == canonical(&resolved))
                && let Some(to) = redirect["to"].as_str()
            {
                resolved = to.to_string();
            }
        }
    }
    let pages = json.pointer("/query/pages").and_then(Value::as_array)?;
    let identity = resolved
        .split_once(" (")
        .map_or(resolved.as_str(), |(name, _)| name);
    let page = wiki_candidate(pages, identity)?;
    let title = page["title"].as_str()?.to_owned();
    let mut url = url::Url::parse(&format!("https://{language}.wikipedia.org/wiki/")).ok()?;
    url.path_segments_mut()
        .ok()?
        .pop_if_empty()
        .push(&title.replace(' ', "_"));
    Some(Biography {
        title,
        text: page["extract"].as_str()?.chars().take(8000).collect(),
        url: url.into(),
    })
}
#[cfg(test)]
fn expand_biography(
    bio: Biography,
    fetch: &mut impl FnMut(&url::Url) -> FetchAnswer,
) -> Answer<Biography> {
    expand_biography_in(bio, "it", fetch)
}
fn expand_biography_in(
    bio: Biography,
    language: &str,
    fetch: &mut impl FnMut(&url::Url) -> FetchAnswer,
) -> Answer<Biography> {
    if bio.text.chars().count() >= 300 {
        return Ok(Some(bio));
    }
    // Expand only a page whose artist identity and musical subject were validated.
    // A short lead is still a useful result if this optional request fails.
    let mut url = url::Url::parse(&format!("https://{language}.wikipedia.org/w/api.php")).unwrap();
    url.query_pairs_mut().extend_pairs([
        ("action", "query"),
        ("titles", bio.title.as_str()),
        ("prop", "extracts|pageprops"),
        ("ppprop", "disambiguation"),
        ("explaintext", "1"),
        ("exsectionformat", "plain"),
        ("exchars", "1200"),
        ("exlimit", "1"),
        ("format", "json"),
        ("formatversion", "2"),
    ]);
    match fetch(&url) {
        Ok(Some(json)) => {
            if let Some(full) = wiki_biography_in(&json, &bio.title, language)
                && full.title == bio.title
                && full.text.chars().count() > bio.text.chars().count()
            {
                return Ok(Some(full));
            }
        }
        Err(FetchError::Cancelled) => return Err(FetchError::Cancelled.into()),
        _ => {}
    }
    Ok(Some(bio))
}
fn fetch_artist_using(
    artist: &str,
    fetch: &mut impl FnMut(&url::Url) -> FetchAnswer,
) -> Answer<Biography> {
    fetch_artist_using_in(artist, "it", fetch)
}
fn fetch_artist_using_in(
    artist: &str,
    language: &str,
    fetch: &mut impl FnMut(&url::Url) -> FetchAnswer,
) -> Answer<Biography> {
    let lookup = lookup_artist(artist);
    let artist = lookup.as_str();
    let mut error = None;
    for mode in 0..3 {
        let mut url =
            url::Url::parse(&format!("https://{language}.wikipedia.org/w/api.php")).unwrap();
        url.query_pairs_mut().extend_pairs([
            ("action", "query"),
            ("prop", "extracts|pageprops"),
            ("exintro", "1"),
            ("explaintext", "1"),
            ("ppprop", "disambiguation"),
            ("format", "json"),
            ("formatversion", "2"),
        ]);
        if mode == 2 {
            url.query_pairs_mut().extend_pairs([
                ("generator", "search"),
                (
                    "gsrsearch",
                    &format!("intitle:\"{}\"", artist_base(artist).replace('"', " ")),
                ),
                ("gsrnamespace", "0"),
                ("gsrlimit", "10"),
            ]);
        } else if mode == 1 {
            // Homonyms such as Area can bury the musician below geographic results.
            let qualifiers = if language == "en" {
                [
                    "band",
                    "singer",
                    "songwriter",
                    "musician",
                    "composer",
                    "rapper",
                    "disc jockey",
                    "orchestra",
                ]
            } else {
                [
                    "gruppo musicale",
                    "cantante",
                    "cantautore",
                    "musicista",
                    "compositore",
                    "rapper",
                    "disc jockey",
                    "orchestra",
                ]
            };
            let titles = qualifiers
                .map(|qualifier| format!("{artist} ({qualifier})"))
                .join("|");
            url.query_pairs_mut()
                .extend_pairs([("titles", titles.as_str()), ("redirects", "1")]);
        } else {
            url.query_pairs_mut()
                .extend_pairs([("titles", artist), ("redirects", "1")]);
        }
        match fetch(&url) {
            Ok(Some(json)) => {
                if json.get("error").is_some() {
                    error = Some("Wikipedia non è disponibile in questo momento.".into());
                } else if let Some(bio) = wiki_biography_in(&json, artist, language) {
                    return expand_biography_in(bio, language, fetch);
                }
            }
            Ok(None) => {}
            Err(FetchError::Unavailable(e)) => error = Some(e),
            Err(e) => return Err(e.into()),
        }
    }
    error.map_or(Ok(None), Err)
}
fn lyric_candidate<'a>(records: &'a [Value], q: &Query) -> Option<&'a Value> {
    lyric_candidate_within(records, q, 3.0, true)
}
fn live_album(s: &str) -> bool {
    s.split(|c: char| !c.is_alphanumeric()).any(|word| {
        ["live", "concert", "concerto"]
            .iter()
            .any(|w| word.eq_ignore_ascii_case(w))
    })
}
fn lyric_candidate_within<'a>(
    records: &'a [Value],
    q: &Query,
    tolerance: f64,
    timed: bool,
) -> Option<&'a Value> {
    records
        .iter()
        .filter_map(|r| {
            let artist = r["artistName"].as_str()?;
            if artist_key(artist) != artist_key(&q.artist)
                || canonical(&without_feature_credits(&lookup_title(r["trackName"].as_str()?)).0)
                    != canonical(&without_feature_credits(&lookup_title(&q.title)).0)
            {
                return None;
            }
            let duration = r["duration"].as_f64()?;
            if q.duration == 0
                || !duration.is_finite()
                || duration <= 0.0
                || (duration - q.duration as f64).abs() > tolerance
            {
                return None;
            }
            let album = r["albumName"].as_str().unwrap_or("");
            if live_album(album) != live_album(&q.album)
                || (timed && live_album(album) && canonical(album) != canonical(&q.album))
                || (!timed && r["instrumental"].as_bool() == Some(true))
            {
                return None;
            }
            if r["instrumental"].as_bool() != Some(true)
                && r["plainLyrics"].as_str().unwrap_or("").is_empty()
                && parse_lrc(r["syncedLyrics"].as_str().unwrap_or(""))
                    .iter()
                    .filter(|l| !l.text.is_empty())
                    .count()
                    < 2
            {
                return None;
            }
            let album_match = !q.album.is_empty() && canonical(album) == canonical(&q.album);
            let synced = usize::from(
                parse_lrc(r["syncedLyrics"].as_str().unwrap_or(""))
                    .iter()
                    .filter(|l| !l.text.is_empty())
                    .count()
                    >= 2,
            );
            let difference = (duration - q.duration as f64).abs();
            Some((
                (
                    canonical(artist) == canonical(&q.artist),
                    album_match,
                    u32::MAX - difference.round() as u32,
                    synced,
                    u32::MAX - (difference * 1000.0) as u32,
                ),
                r,
            ))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, r)| r)
}
fn lyrics_record(r: &Value) -> Lyrics {
    let mut lines = parse_lrc(r["syncedLyrics"].as_str().unwrap_or(""));
    if lines.iter().filter(|l| !l.text.is_empty()).count() < 2 {
        lines.clear();
    }
    Lyrics {
        lines,
        plain: r["plainLyrics"]
            .as_str()
            .unwrap_or("")
            .chars()
            .take(100_000)
            .collect(),
        instrumental: r["instrumental"].as_bool().unwrap_or(false),
        url: r["id"].as_u64().map_or_else(
            || "https://lrclib.net".into(),
            |id| format!("https://lrclib.net/api/get/{id}"),
        ),
    }
}
fn plain_lyrics_record(r: &Value) -> Lyrics {
    let mut song = lyrics_record(r);
    if song.plain.trim().is_empty() {
        song.plain = song
            .lines
            .iter()
            .filter(|l| !l.text.is_empty())
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .chars()
            .take(100_000)
            .collect();
    }
    song.lines.clear();
    song
}
// Feature credits are alternative lookup identities, never tag edits or arbitrary aliases.
fn feature_credit(s: &str) -> Option<&str> {
    let s = s.trim();
    for prefix in ["featuring", "feat.", "feat", "ft.", "ft"] {
        if s.get(..prefix.len())
            .is_some_and(|v| v.eq_ignore_ascii_case(prefix))
            && s.get(prefix.len()..)
                .is_some_and(|tail| tail.starts_with(char::is_whitespace))
        {
            return Some(s[prefix.len()..].trim());
        }
    }
    None
}
fn without_feature_credits(s: &str) -> (String, Vec<String>) {
    let mut text = String::new();
    let mut guests = Vec::new();
    let mut rest = s;
    while let Some((index, open)) = rest.char_indices().find(|(_, c)| matches!(c, '(' | '[')) {
        text.push_str(&rest[..index]);
        let tail = &rest[index + 1..];
        let close = if open == '(' { ')' } else { ']' };
        let Some(end) = tail.find(close) else {
            rest = &rest[index..];
            break;
        };
        if let Some(guest) = feature_credit(&tail[..end]).filter(|g| !g.is_empty()) {
            guests.push(guest.to_owned());
        } else {
            text.push_str(&rest[index..index + end + 2]);
        }
        rest = &tail[end + 1..];
    }
    text.push_str(rest);
    let lower = text.to_ascii_lowercase();
    let split = [" featuring ", " feat. ", " feat ", " ft. ", " ft "]
        .iter()
        .filter_map(|marker| lower.find(marker).map(|i| (i, marker.len())))
        .min_by_key(|(i, _)| *i);
    if let Some((i, length)) = split {
        let guest = text[i + length..].trim();
        if !guest.is_empty() {
            guests.push(guest.to_owned());
        }
        text.truncate(i);
    }
    (
        text.split_whitespace().collect::<Vec<_>>().join(" "),
        guests,
    )
}
fn lyric_queries(q: &Query) -> Vec<Query> {
    let (title, mut guests) = without_feature_credits(&lookup_title(&q.title));
    let (artist, artist_guests) = without_feature_credits(&q.artist);
    guests.extend(artist_guests);
    let mut queries = Vec::new();
    for artist in std::iter::once(artist).chain(guests).take(4) {
        if artist.is_empty()
            || queries
                .iter()
                .any(|v: &Query| artist_key(&v.artist) == artist_key(&artist))
        {
            continue;
        }
        queries.push(Query {
            artist,
            title: title.clone(),
            ..q.clone()
        });
    }
    queries
}
fn fetch_lyrics(q: &Query, cancelled: &dyn Fn() -> bool) -> Answer<Lyrics> {
    fetch_lyrics_using(q, &mut |url| get(url, cancelled))
}
fn fetch_lyrics_using(
    q: &Query,
    fetch: &mut impl FnMut(&url::Url) -> FetchAnswer,
) -> Answer<Lyrics> {
    if q.duration == 0 {
        return Ok(None);
    }
    let mut error = None;
    let mut plain = None;
    for q in &lyric_queries(q) {
        let artist = lookup_artist(&q.artist);
        for mode in 0..3 {
            if mode > 0 && q.album.is_empty() {
                continue;
            }
            let search = mode != 2;
            let mut url = url::Url::parse(if search {
                "https://lrclib.net/api/search"
            } else {
                "https://lrclib.net/api/get"
            })
            .unwrap();
            let title = lookup_title(&q.title);
            url.query_pairs_mut().extend_pairs([
                (
                    "artist_name",
                    if search {
                        artist_base(&artist)
                    } else {
                        artist.as_str()
                    },
                ),
                ("track_name", title.as_str()),
            ]);
            if mode > 0 {
                url.query_pairs_mut().append_pair("album_name", &q.album);
            }
            if !search {
                url.query_pairs_mut()
                    .extend_pairs([("duration", &q.duration.to_string())]);
            }
            match fetch(&url) {
                Ok(Some(json)) => {
                    let records = if search {
                        json.as_array().map(Vec::as_slice).unwrap_or(&[])
                    } else {
                        std::slice::from_ref(&json)
                    };
                    if let Some(record) = lyric_candidate(records, q) {
                        return Ok(Some(lyrics_record(record)));
                    }
                    if plain.is_none()
                        && let Some(record) = lyric_candidate_within(records, q, 12.0, false)
                    {
                        plain = Some(plain_lyrics_record(record));
                    }
                }
                Ok(None) => {}
                Err(FetchError::Unavailable(e)) => error = Some(e),
                Err(e) => return Err(e.into()),
            }
        }
        if plain.is_some() {
            return Ok(plain);
        }
    }
    if plain.is_some() {
        Ok(plain)
    } else {
        error.map_or(Ok(None), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn biographies_follow_trusted_language_links_and_fall_back_to_english() {
        for (locale, host, title) in [
            ("ja", "ja", "架空のバンド"),
            ("zh-hans", "zh", "虚构乐队"),
            ("zh-hant", "zh", "虛構樂團"),
            ("pt-br", "pt", "Banda Exemplo"),
        ] {
            let mut calls = 0;
            let bio = fetch_artist_localized("Demo Band", locale, &mut |url| {
                calls += 1;
                if calls == 1 { assert_eq!(url.host_str(),Some("en.wikipedia.org")); return Ok(Some(serde_json::json!({"query":{"pages":[{"title":"Demo Band","extract":"A fictional music band for this test. ".repeat(15)}]}}))); }
                if calls == 2 { assert!(url.query_pairs().any(|(k,v)| k=="lllang" && v==host)); return Ok(Some(serde_json::json!({"query":{"pages":[{"langlinks":[{"lang":host,"title":title}]}]}}))); }
                assert_eq!(url.host_str(),Some(format!("{host}.wikipedia.org").as_str()));
                if host=="zh" { assert!(url.query_pairs().any(|(k,v)| k=="variant" && v==locale)); }
                Ok(Some(serde_json::json!({"query":{"pages":[{"title":title,"extract":"Localized synthetic biography."}]}})))
            }).unwrap().unwrap();
            assert_eq!(bio.title, title);
            assert!(
                bio.url
                    .starts_with(&format!("https://{host}.wikipedia.org/"))
            );
            assert_eq!(calls, 3);
        }
        let mut calls = 0;
        let bio = fetch_artist_localized("Demo Band", "de", &mut |_| {
            calls += 1;
            Ok(Some(if calls==1 { serde_json::json!({"query":{"pages":[{"title":"Demo Band","extract":"A fictional music band for this test. ".repeat(15)}]}}) } else {serde_json::json!({"query":{"pages":[{"langlinks":[]}]}})}))
        }).unwrap().unwrap();
        assert_eq!(calls, 2);
        assert!(bio.url.starts_with("https://en.wikipedia.org/"));
    }
    #[test]
    fn changing_biography_language_reuses_lyrics_and_rejects_old_responses() {
        let (tx, requests) = mpsc::channel();
        let (_responses, rx) = mpsc::channel();
        let mut info = Info {
            tx,
            rx,
            generation: Arc::new(AtomicU64::new(0)),
            query: None,
            language: String::new(),
            artists: HashMap::new(),
            songs: HashMap::new(),
            artist: State::Empty,
            lyrics: State::Empty,
        };
        let q = beatles_query();
        crate::i18n::set("it");
        info.update(true, Some(q.clone()));
        let it = requests.recv().unwrap();
        let bio = Biography {
            title: "Fixture".into(),
            text: "Synthetic".into(),
            url: "https://it.wikipedia.org/wiki/Fixture".into(),
        };
        info.apply(Response {
            epoch: it.epoch,
            query: q.clone(),
            payload: Payload::Artist(Ok(Some(bio.clone()))),
        });
        info.apply(Response {
            epoch: it.epoch,
            query: q.clone(),
            payload: Payload::Lyrics(Ok(Some(Lyrics {
                lines: vec![],
                plain: "Synthetic lyrics".into(),
                instrumental: false,
                url: String::new(),
            }))),
        });
        crate::i18n::set("de");
        info.update(true, Some(q.clone()));
        let de = requests.recv().unwrap();
        assert_eq!(de.language, "de");
        assert!(de.artist);
        assert!(!de.lyrics);
        assert!(matches!(info.lyrics, State::Ready(_)));
        info.apply(Response {
            epoch: it.epoch,
            query: q.clone(),
            payload: Payload::Artist(Ok(Some(bio))),
        });
        assert!(matches!(info.artist, State::Loading));
        crate::i18n::set("it");
        info.update(true, Some(q));
        assert!(matches!(info.artist, State::Ready(_)));
        assert!(requests.try_recv().is_err());
        crate::i18n::set("en");
    }
    #[test]
    #[ignore = "explicit read-only Wikipedia language integration"]
    fn live_biography_english_and_german_follow_the_same_artist() {
        for locale in ["en", "de"] {
            let bio = fetch_artist_localized("The Beatles", locale, &mut |url| get(url, &|| false))
                .unwrap()
                .unwrap();
            assert!(
                bio.url
                    .starts_with(&format!("https://{locale}.wikipedia.org/"))
            );
            assert!(bio.text.chars().count() > 100);
            println!(
                "Biography locale={locale} title={} chars={}",
                bio.title,
                bio.text.chars().count()
            );
        }
    }
    #[test]
    fn foobar_style_metadata_cleanup_preserves_recording_identity() {
        let mut t = Track::placeholder("/synthetic/example.flac".into());
        t.title = "07 Eric Woolfson - Golden Key (Official Audio)".into();
        let q = Query::from_track(&t).unwrap();
        assert_eq!(q.artist, "Eric Woolfson");
        assert_eq!(q.title, "Golden Key");
        assert!(t.artist.is_empty());
        t.artist = "Known Artist".into();
        t.title = "07 Song (Live)".into();
        let q = Query::from_track(&t).unwrap();
        assert_eq!(q.title, "Song (Live)");
        t.title.clear();
        assert_eq!(Query::from_track(&t).unwrap().artist, "Known Artist");
        t.artist.clear();
        assert!(Query::from_track(&t).is_none());
        assert_eq!(lookup_title("7 Years"), "7 Years");
        assert_eq!(lookup_title("03 Song (Remix)"), "Song (Remix)");
    }
    #[test]
    fn metadata_cache_shares_payload_and_evicts_only_oldest_entry() {
        let mut cache: Cache<String, Biography> = HashMap::new();
        let bio = Arc::new(Biography {
            title: "Fixture".into(),
            text: "Synthetic biography".into(),
            url: String::new(),
        });
        for i in 0..128 {
            cache.insert(
                i.to_string(),
                (
                    Ok(Some(bio.clone())),
                    Instant::now() - Duration::from_secs(128 - i),
                ),
            );
        }
        let copy = cached(&cache, &"127".to_string())
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(Arc::ptr_eq(&copy, &bio));
        evict_oldest(&mut cache);
        assert_eq!(cache.len(), 127);
        assert!(!cache.contains_key("0"));
        assert!(cache.contains_key("127"));
    }
    #[test]
    fn italian_articles_match_only_complete_artist_names_and_can_be_inverted() {
        for (tag, name) in [
            ("Gli Squallor", "Squallor"),
            ("Squallor, Gli", "Squallor"),
            ("Il Volo", "Volo"),
            ("Lo Stato Sociale", "Stato Sociale"),
            ("La Crus", "Crus"),
            ("I Nomadi", "Nomadi"),
            ("Le Orme", "Orme"),
            ("L'Officina", "Officina"),
            ("L’Officina", "Officina"),
        ] {
            assert_eq!(artist_key(tag), artist_key(name));
        }
        assert_eq!(lookup_artist("Squallor, Gli"), "Gli Squallor");
        assert_eq!(lookup_artist("Officina, L'"), "L'Officina");
        for name in ["The The", "Gli", "Le Le", "I", "I Am", "Elio", "Glitter"] {
            assert_eq!(artist_key(name), canonical(name));
        }
        assert_ne!(artist_key("Gli Squallor Tribute"), artist_key("Squallor"));
    }
    #[test]
    fn vocal_group_redirect_and_lyrics_search_accept_the_name_without_article() {
        let json = serde_json::json!({"query":{"redirects":[{"from":"Gli Squallor","to":"Squallor"}],"pages":[{"title":"Squallor","extract":"Un gruppo vocale immaginario per questa regressione."}]}});
        assert_eq!(
            wiki_biography(&json, "Gli Squallor").unwrap().title,
            "Squallor"
        );
        let q = Query {
            artist: "Gli Squallor".into(),
            title: "Brano di collaudo".into(),
            album: "Album sintetico".into(),
            duration: 241,
        };
        let song=fetch_lyrics_using(&q,&mut |url| {
            assert_eq!(url.path(),"/api/search");
            assert!(url.query_pairs().any(|(k,v)|k=="artist_name" && v=="Squallor"));
            Ok(Some(serde_json::json!([{"artistName":"Squallor","trackName":q.title,"albumName":q.album,"duration":241,"plainLyrics":"Versi sintetici di verifica"}])))
        }).unwrap().unwrap();
        assert!(!song.plain.is_empty());
    }
    #[test]
    fn literal_artist_name_wins_when_article_normalization_has_multiple_candidates() {
        let q = Query {
            artist: "Le Example".into(),
            title: "Prova".into(),
            album: "Studio".into(),
            duration: 200,
        };
        let records = serde_json::json!([{"id":1,"artistName":"Le Example","trackName":"Prova","duration":201,"plainLyrics":"Testo fittizio"},{"id":2,"artistName":"Example","trackName":"Prova","duration":200,"plainLyrics":"Altro testo fittizio"}]);
        assert_eq!(
            lyric_candidate(records.as_array().unwrap(), &q).unwrap()["id"],
            1
        );
        let pages = serde_json::json!([{"title":"Le Example","extract":"Un gruppo vocale fittizio."},{"title":"Example","extract":"Un gruppo vocale fittizio."}]);
        assert_eq!(
            wiki_candidate(pages.as_array().unwrap(), &q.artist).unwrap()["title"],
            "Le Example"
        );
    }
    fn short_artist_page() -> Value {
        serde_json::json!({"query":{"pages":[{"title":"Fink (musicista)","extract":"Un musicista immaginario usato per il test."}]}})
    }
    #[test]
    fn short_leads_are_valid_but_empty_unrelated_and_non_musical_pages_are_not() {
        let mut json = short_artist_page();
        assert!(wiki_biography(&json, "Fink").is_some());
        for text in ["", "   ", "Una voce geografica."] {
            json["query"]["pages"][0]["extract"] = text.into();
            assert!(wiki_biography(&json, "Fink").is_none());
        }
        assert!(wiki_biography(&short_artist_page(), "Pink").is_none());
    }
    #[test]
    fn short_qualified_artist_is_expanded_after_disambiguation_without_changing_identity() {
        let mut calls = 0;
        let bio = fetch_artist_using("Fink", &mut |url| {
            calls += 1;
            if calls == 1 {
                return Ok(Some(serde_json::json!({"query":{"pages":[{"title":"Fink","pageprops":{"disambiguation":""}}]}})));
            }
            if calls == 2 {
                return Ok(Some(short_artist_page()));
            }
            assert_eq!(calls, 3);
            assert!(url.query_pairs().any(|(k,v)|k=="titles" && v=="Fink (musicista)"));
            assert!(!url.query_pairs().any(|(k,_)|k=="exintro" || k=="generator"));
            assert!(url.query_pairs().any(|(k,v)|k=="exchars" && v=="1200"));
            let mut json=short_artist_page();
            json["query"]["pages"][0]["extract"]="Un musicista immaginario per il collaudo della biografia estesa. ".repeat(10).into();
            Ok(Some(json))
        }).unwrap().unwrap();
        assert_eq!(calls, 3);
        assert_eq!(bio.title, "Fink (musicista)");
        assert!(bio.text.chars().count() > 300);
        assert_eq!(bio.url, "https://it.wikipedia.org/wiki/Fink_(musicista)");
    }
    #[test]
    fn failed_optional_expansion_preserves_the_verified_lead_and_cancellation_stops() {
        let bio = wiki_biography(&short_artist_page(), "Fink").unwrap();
        for error in [
            FetchError::Unavailable("Offline".into()),
            FetchError::RateLimited,
        ] {
            let mut calls = 0;
            let result = expand_biography(bio.clone(), &mut |_| {
                calls += 1;
                Err(error.clone())
            })
            .unwrap()
            .unwrap();
            assert_eq!(calls, 1);
            assert_eq!(result.text, bio.text);
        }
        for title in ["Fink (software)", "Other Artist (musicista)"] {
            let mut json = short_artist_page();
            json["query"]["pages"][0]["title"] = title.into();
            json["query"]["pages"][0]["extract"] =
                "Un musicista citato in una pagina non corrispondente. "
                    .repeat(10)
                    .into();
            let result = expand_biography(bio.clone(), &mut |_| Ok(Some(json.clone())))
                .unwrap()
                .unwrap();
            assert_eq!(result.text, bio.text);
        }
        assert!(expand_biography(bio, &mut |_| Err(FetchError::Cancelled)).is_err());
    }
    #[test]
    fn verified_redirect_can_resolve_to_a_qualified_artist_title() {
        let json = serde_json::json!({"query":{"redirects":[{"from":"An Artist Alias","to":"Fink (musicista)"}],"pages":[{"title":"Fink (musicista)","extract":"Un musicista immaginario per il test."}]}});
        assert_eq!(
            wiki_biography(&json, "An Artist Alias").unwrap().title,
            "Fink (musicista)"
        );
    }
    #[test]
    fn rock_groups_and_qualified_homonyms_are_biographies_but_geography_is_not() {
        let rock = serde_json::json!([{"title":"The Alan Parsons Project","extract":"Un gruppo rock progressivo immaginario per questo test, con una storia discografica descritta soltanto da dati sintetici."}]);
        assert!(wiki_candidate(rock.as_array().unwrap(), "Alan Parsons Project").is_some());
        let mut calls = 0;
        let bio = fetch_artist_using("Area", &mut |url| {
            calls += 1;
            if calls == 1 {
                return Ok(Some(serde_json::json!({"query":{"pages":[{"title":"Area","extract":"Una misura geometrica della superficie. ".repeat(5)}]}})));
            }
            assert!(url.query_pairs().any(|(k,v)|k=="titles" && v.contains("Area (gruppo musicale)")));
            Ok(Some(serde_json::json!({"query":{"pages":[{"title":"Area (gruppo musicale)","extract":"Un gruppo musicale immaginario usato per il test. ".repeat(10)}]}})))
        }).unwrap().unwrap();
        assert_eq!(bio.title, "Area (gruppo musicale)");
        assert_eq!(calls, 2);
    }
    #[test]
    fn unicode_accents_width_and_apostrophes_do_not_hide_an_exact_song() {
        assert_eq!(artist_key("Amalia Gre\u{301}"), artist_key("Amalia Gré"));
        assert_eq!(lookup_artist("Amalia Gre\u{301}"), "Amalia Gré");
        let pages = serde_json::json!([{"title":"Amalia Gré","extract":"Una cantautrice immaginaria per il test della forma femminile e degli accenti nei nomi con una biografia sintetica."}]);
        assert!(wiki_candidate(pages.as_array().unwrap(), "Amalia Gre\u{301}").is_some());
        assert_eq!(canonical("Ｋｈｏｒａｋｈａｎé"), canonical("Khorakhane'"));
        let q = Query {
            artist: "Fabrizio De André".into(),
            title: "Khorakhane' (a forza di essere vento)".into(),
            album: "Studio".into(),
            duration: 333,
        };
        let records = serde_json::json!([{"artistName":"Fabrizio De Andrè","trackName":"Khorakhané (A forza di essere vento)","duration":333,"plainLyrics":"Versi inventati per il test"}]);
        assert!(lyric_candidate(records.as_array().unwrap(), &q).is_some());
        assert!(lyric_candidate(records.as_array().unwrap(), &beatles_query()).is_none());
    }
    #[test]
    fn temporary_retry_is_bounded_and_never_retries_rate_limits_or_cancellation() {
        let mut calls = 0;
        assert!(
            retry_temporary(
                &mut || {
                    calls += 1;
                    if calls == 1 {
                        Err(FetchError::Temporary("503".into()))
                    } else {
                        Ok(None)
                    }
                },
                &|| false
            )
            .is_ok()
        );
        assert_eq!(calls, 2);
        calls = 0;
        assert!(
            retry_temporary(
                &mut || {
                    calls += 1;
                    Err(FetchError::Temporary("503".into()))
                },
                &|| false
            )
            .is_err()
        );
        assert_eq!(calls, 2);
        for error in [
            FetchError::RateLimited,
            FetchError::Cancelled,
            FetchError::Unavailable("Invalid response".into()),
        ] {
            calls = 0;
            assert!(
                retry_temporary(
                    &mut || {
                        calls += 1;
                        Err(error.clone())
                    },
                    &|| false
                )
                .is_err()
            );
            assert_eq!(calls, 1);
        }
        calls = 0;
        assert!(matches!(
            retry_temporary(
                &mut || {
                    calls += 1;
                    Err(FetchError::Temporary("503".into()))
                },
                &|| true
            ),
            Err(FetchError::Cancelled)
        ));
        assert_eq!(calls, 1);
    }
    #[test]
    fn album_search_recovers_a_record_hidden_by_the_twenty_result_limit() {
        let mut calls = 0;
        let song = fetch_lyrics_using(&beatles_query(), &mut |url| {
            calls += 1;
            assert_eq!(url.path(), "/api/search");
            if calls == 1 {
                let mut record = synthetic_record();
                record["duration"] = 999.into();
                return Ok(Some(Value::Array(vec![record; 20])));
            }
            assert!(
                url.query_pairs()
                    .any(|(k, v)| k == "album_name" && v == "A Hard Day's Night")
            );
            Ok(Some(serde_json::json!([synthetic_record()])))
        })
        .unwrap()
        .unwrap();
        assert_eq!(song.lines.len(), 2);
        assert_eq!(calls, 2);
    }
    #[test]
    fn nearby_duration_provides_plain_text_only_after_searching_for_compatible_timings() {
        let mut record = synthetic_record();
        record["duration"] = 150.into();
        record["plainLyrics"] = Value::Null;
        let mut calls = 0;
        let song = fetch_lyrics_using(&beatles_query(), &mut |_| {
            calls += 1;
            Ok(Some(if calls < 3 {
                serde_json::json!([record])
            } else {
                Value::Null
            }))
        })
        .unwrap()
        .unwrap();
        assert_eq!(calls, 3);
        assert!(song.lines.is_empty());
        assert_eq!(song.plain, "Luce di prova\nSeconda riga inventata");
        record["duration"] = 157.into();
        assert!(lyric_candidate_within(&[record.clone()], &beatles_query(), 12.0, false).is_none());
        record["duration"] = 150.into();
        record["trackName"] = "If I Fell (Live)".into();
        assert!(lyric_candidate_within(&[record], &beatles_query(), 12.0, false).is_none());
    }
    #[test]
    fn different_concerts_cannot_supply_karaoke_even_when_the_durations_match() {
        let mut q = beatles_query();
        q.album = "Live in Alpha".into();
        let mut record = synthetic_record();
        assert!(lyric_candidate(&[record.clone()], &q).is_none());
        record["albumName"] = "Live in Beta".into();
        assert!(lyric_candidate(&[record.clone()], &q).is_none());
        assert!(lyric_candidate_within(&[record.clone()], &q, 12.0, false).is_some());
        record["albumName"] = "Live in Alpha".into();
        assert!(lyric_candidate(&[record], &q).is_some());
    }
    #[test]
    fn exact_album_and_duration_outrank_conflicting_timed_lyrics() {
        let q = beatles_query();
        let mut timed = synthetic_record();
        timed["duration"] = 145.into();
        let instrumental = serde_json::json!({"id":2,"artistName":q.artist,"trackName":q.title,"albumName":q.album,"duration":144,"instrumental":true});
        assert_eq!(
            lyric_candidate(&[timed.clone(), instrumental], &q).unwrap()["id"],
            2
        );
        timed["albumName"] = "Another edition".into();
        let plain = serde_json::json!({"id":3,"artistName":q.artist,"trackName":q.title,"albumName":q.album,"duration":145,"plainLyrics":"Versi inventati"});
        assert_eq!(lyric_candidate(&[timed, plain], &q).unwrap()["id"], 3);
    }
    #[test]
    #[ignore = "Private metadata sample; requires Internet and AUREA_AUDIT_SAMPLE / AUREA_AUDIT_REPORT"]
    fn live_library_sample() {
        let sample = std::env::var("AUREA_AUDIT_SAMPLE").expect("Provide sanitized query JSON");
        let report = std::env::var("AUREA_AUDIT_REPORT").expect("Provide a private output path");
        let queries: Vec<Query> = serde_json::from_slice(&std::fs::read(sample).unwrap()).unwrap();
        assert!(!queries.is_empty() && queries.len() <= 100);
        let mut info = Info::default();
        let mut rows = Vec::new();
        for q in &queries {
            assert!(!q.artist.is_empty() && !q.title.is_empty() && q.duration > 0);
            assert!(
                [&q.artist, &q.title, &q.album]
                    .iter()
                    .all(|s| s.chars().count() <= 256)
            );
            let deadline = Instant::now() + Duration::from_secs(130);
            info.update(true, Some(q.clone()));
            while (matches!(info.artist, State::Loading) || matches!(info.lyrics, State::Loading))
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(50));
                info.update(true, Some(q.clone()));
            }
            let bio = match &info.artist {
                State::Ready(b) => {
                    serde_json::json!({"status":"ready","title":b.title,"url":b.url,"chars":b.text.chars().count()})
                }
                State::Missing => serde_json::json!({"status":"missing"}),
                State::Error(e) => serde_json::json!({"status":"error","error":e}),
                state => panic!("Biography did not complete: {state:?}"),
            };
            let lyrics = match &info.lyrics {
                State::Ready(l) => {
                    serde_json::json!({"status":if l.instrumental {"instrumental"} else if l.lines.is_empty() {"plain"} else {"synced"},"lines":l.lines.len(),"plain_chars":l.plain.chars().count(),"url":l.url})
                }
                State::Missing => serde_json::json!({"status":"missing"}),
                State::Error(e) => serde_json::json!({"status":"error","error":e}),
                state => panic!("Lyrics did not complete: {state:?}"),
            };
            println!(
                "{}/{} {} / {}: biography={} lyrics={}",
                rows.len() + 1,
                queries.len(),
                q.artist,
                q.title,
                bio["status"],
                lyrics["status"]
            );
            let rate_limited = [&bio, &lyrics]
                .iter()
                .any(|v| v["error"].as_str() == Some("Servizio occupato. Riprova più tardi."));
            rows.push(serde_json::json!({"artist":q.artist,"title":q.title,"album":q.album,"duration":q.duration,"biography":bio,"lyrics":lyrics}));
            std::fs::write(&report, serde_json::to_vec_pretty(&rows).unwrap()).unwrap();
            assert!(!rate_limited, "Audit stopped: provider rate limit");
            // Audits are intentionally slower than interactive use. Never bypass a 429.
            std::thread::sleep(Duration::from_secs(3));
        }
    }
    fn beatles_query() -> Query {
        Query {
            artist: "The Beatles".into(),
            title: "If I Fell".into(),
            album: "A Hard Day's Night".into(),
            duration: 144,
        }
    }
    fn synthetic_record() -> Value {
        serde_json::json!({"id":1,"artistName":"The Beatles","trackName":"If I Fell","albumName":"A Hard Day's Night","duration":144,"plainLyrics":"Testo sintetico di collaudo","syncedLyrics":"[00:01]Luce di prova\n[00:12]Seconda riga inventata"})
    }
    #[test]
    fn wikipedia_requests_exact_title_before_search_and_resolves_aliases() {
        let mut calls = 0;
        let answer=fetch_artist_using("Beatles",&mut |url|{
            calls+=1;
            assert!(url.query_pairs().any(|(k,v)|k=="titles" && v=="Beatles"));
            assert!(url.query_pairs().any(|(k,v)|k=="redirects" && v=="1"));
            Ok(Some(serde_json::json!({"query":{"redirects":[{"from":"Beatles","to":"The Beatles"}],"pages":[{"title":"The Beatles","extract":"Fixture di un gruppo musicale per verificare l'API. ".repeat(10)}]}})))
        }).unwrap().unwrap();
        assert_eq!(calls, 1);
        assert_eq!(answer.title, "The Beatles");
    }
    #[test]
    fn wikipedia_missing_title_falls_back_without_accepting_related_album() {
        let mut calls = 0;
        let answer=fetch_artist_using("The Beatles",&mut |url|{
            calls+=1;
            if calls<=2 {return Ok(Some(serde_json::json!({"query":{"pages":[{"title":"The Beatles","missing":true}]}})));}
            assert!(url.query_pairs().any(|(k,_)|k=="generator"));
            Ok(Some(serde_json::json!({"query":{"pages":[{"title":"The Beatles Anthology","extract":"Un album musicale inventato per il test. ".repeat(5)},{"title":"The Beatles","extract":"Un gruppo musicale nella fixture. ".repeat(10)}]}})))
        }).unwrap().unwrap();
        assert_eq!(calls, 3);
        assert_eq!(answer.title, "The Beatles");
    }
    #[test]
    fn explicit_feature_artist_recovers_lyrics_without_changing_tags_or_version() {
        let q = Query {
            artist: "Eminem".into(),
            title: "Purple Hills (feat. D12)".into(),
            album: "Best of Eminem".into(),
            duration: 304,
        };
        let record = serde_json::json!({"id":42,"artistName":"D12","trackName":"Purple Hills",
            "albumName":"Fixture compilation","duration":304,"plainLyrics":"Synthetic test lines"});
        let mut calls = 0;
        let song = fetch_lyrics_using(&q, &mut |url| {
            calls += 1;
            let params: HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(params["track_name"], "Purple Hills");
            if params["artist_name"] == "D12" {
                Ok(Some(serde_json::json!([record])))
            } else {
                assert_eq!(params["artist_name"], "Eminem");
                Ok(Some(serde_json::json!([])))
            }
        })
        .unwrap()
        .unwrap();
        assert_eq!(calls, 4);
        assert!(!song.plain.is_empty());
        assert!(song.lines.is_empty());
        assert_eq!(q.title, "Purple Hills (feat. D12)");
        assert_eq!(q.artist, "Eminem");
        let candidates = lyric_queries(&q);
        assert_eq!(candidates.len(), 2);
        assert!(lyric_candidate(std::slice::from_ref(&record), &candidates[1]).is_some());
        let mut unrelated = record.clone();
        unrelated["artistName"] = "Unrelated artist".into();
        assert!(lyric_candidate(&[unrelated], &candidates[1]).is_none());
        let mut different = record.clone();
        different["trackName"] = "Purple Pills".into();
        assert!(lyric_candidate(&[different], &candidates[1]).is_none());
        let mut live = record;
        live["trackName"] = "Purple Hills (Live)".into();
        assert!(lyric_candidate(&[live], &candidates[1]).is_none());
    }
    #[test]
    fn feature_credits_keep_live_remix_and_other_parenthesized_titles_distinct() {
        for credit in ["feat. D12", "FEAT D12", "ft. D12", "featuring D12"] {
            assert_eq!(
                without_feature_credits(&format!("Song ({credit}) (Live)")),
                ("Song (Live)".into(), vec!["D12".into()])
            );
        }
        assert_eq!(
            without_feature_credits("Song [ft. Guest] (Remix)"),
            ("Song (Remix)".into(), vec!["Guest".into()])
        );
        assert_eq!(
            without_feature_credits("Song (Part One)"),
            ("Song (Part One)".into(), vec![])
        );
        assert_eq!(
            without_feature_credits("Song feat. Guest"),
            ("Song".into(), vec!["Guest".into()])
        );
        let q = Query {
            artist: "Main feat. Guest".into(),
            title: "Song (feat. Guest)".into(),
            album: String::new(),
            duration: 200,
        };
        assert_eq!(lyric_queries(&q).len(), 2, "deduplicate explicit credits");
    }
    #[test]
    #[ignore = "requires LRCLIB network access; public metadata only"]
    fn live_feature_credit_lyrics_lookup() {
        let q = Query {
            artist: "Eminem".into(),
            title: "Purple Hills (feat. D12)".into(),
            album: "Best of Eminem".into(),
            duration: 304,
        };
        let song = fetch_lyrics(&q, &|| false)
            .unwrap()
            .expect("explicit D12 credit must find Purple Hills");
        assert!(!song.plain.is_empty());
        assert!(song.url.starts_with("https://lrclib.net/api/get/"));
        println!(
            "Explicit feature lookup: lyrics found; synchronized={}",
            !song.lines.is_empty()
        );
    }
    #[test]
    fn lyrics_search_succeeds_without_calling_unreliable_exact_endpoint() {
        let answer = fetch_lyrics_using(&beatles_query(), &mut |url| {
            assert_eq!(url.path(), "/api/search");
            Ok(Some(serde_json::json!([synthetic_record()])))
        })
        .unwrap()
        .unwrap();
        assert_eq!(answer.lines.len(), 2);
    }
    #[test]
    fn lyrics_transient_failure_uses_alternative_but_rate_limit_never_does() {
        let mut calls = 0;
        let answer = fetch_lyrics_using(&beatles_query(), &mut |url| {
            calls += 1;
            if calls == 1 {
                assert_eq!(url.path(), "/api/search");
                return Err(FetchError::Unavailable("503".into()));
            }
            if calls == 2 {
                assert!(url.query_pairs().any(|(key, _)| key == "album_name"));
                return Ok(Some(serde_json::json!([])));
            }
            assert_eq!(url.path(), "/api/get");
            Ok(Some(synthetic_record()))
        })
        .unwrap()
        .unwrap();
        assert_eq!(calls, 3);
        assert_eq!(answer.lines.len(), 2);
        calls = 0;
        assert!(
            fetch_lyrics_using(&beatles_query(), &mut |_| {
                calls += 1;
                Err(FetchError::RateLimited)
            })
            .is_err()
        );
        assert_eq!(calls, 1);
    }
    #[test]
    fn lookup_cleans_cosmetic_editions_and_artist_articles_but_keeps_performances() {
        assert_eq!(artist_key("Beatles, The"), artist_key("The Beatles"));
        assert_eq!(lookup_artist("Beatles, The"), "The Beatles");
        assert_ne!(artist_key("The Beatles Tribute"), artist_key("The Beatles"));
        assert_eq!(lookup_title("03. If I Fell - Remastered 2009"), "If I Fell");
        assert_eq!(
            lookup_title("The Narrow Way - Part One (1994 Digital Remasters)"),
            "The Narrow Way - Part One"
        );
        assert_eq!(lookup_title("If I Fell (Live)"), "If I Fell (Live)");
        assert_eq!(lookup_title("1979"), "1979");
        let mut q = beatles_query();
        q.artist = "Beatles".into();
        q.title = "03. If I Fell (2009 Remastered)".into();
        assert!(lyric_candidate(&[synthetic_record()], &q).is_some());
        q.title = "If I Fell (Live)".into();
        assert!(lyric_candidate(&[synthetic_record()], &q).is_none());
    }
    #[test]
    fn retry_clears_negative_cache_without_refetching_ready_biography() {
        let (tx, requests) = mpsc::channel();
        let (_responses, rx) = mpsc::channel();
        let mut info = Info {
            tx,
            rx,
            generation: Arc::new(AtomicU64::new(0)),
            query: None,
            language: String::new(),
            artists: HashMap::new(),
            songs: HashMap::new(),
            artist: State::Empty,
            lyrics: State::Empty,
        };
        let q = beatles_query();
        info.update(true, Some(q.clone()));
        let request = requests.recv().unwrap();
        info.apply(Response {
            epoch: request.epoch,
            query: q.clone(),
            payload: Payload::Artist(Ok(Some(Biography {
                title: "The Beatles".into(),
                text: "Fixture".into(),
                url: String::new(),
            }))),
        });
        info.apply(Response {
            epoch: request.epoch,
            query: q,
            payload: Payload::Lyrics(Ok(None)),
        });
        info.retry();
        let request = requests.recv().unwrap();
        assert!(!request.artist);
        assert!(request.lyrics);
        assert!(matches!(info.artist, State::Ready(_)));
        assert!(matches!(info.lyrics, State::Loading));
    }
    #[test]
    #[ignore = "Read-only reported-track regression; requires Internet"]
    fn live_beatles_if_i_fell_has_biography_and_karaoke() {
        let q = beatles_query();
        let mut info = Info::default();
        let deadline = Instant::now() + Duration::from_secs(45);
        info.update(true, Some(q.clone()));
        while (matches!(info.artist, State::Loading) || matches!(info.lyrics, State::Loading))
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(50));
            info.update(true, Some(q.clone()));
        }
        let State::Ready(bio) = &info.artist else {
            panic!("Beatles biography was not found");
        };
        assert_eq!(bio.title, "The Beatles");
        let State::Ready(lyrics) = &info.lyrics else {
            panic!("If I Fell was not found");
        };
        assert!(lyrics.lines.len() >= 20);
        println!(
            "The Beatles verified; If I Fell synchronized lines: {}",
            lyrics.lines.len()
        );
    }
    #[test]
    fn lrc_supports_offsets_multiple_stamps_unicode_blanks_and_seeking() {
        let lines = parse_lrc(
            "[offset:-500]\n[00:10.25][00:30.500] È luce\n[00:20.00]\n[ar:Demo]\n[00:99]invalid\n[00:40.0]<00:40.0>Fine",
        );
        assert_eq!(
            lines.iter().map(|l| l.time).collect::<Vec<_>>(),
            vec![9.75, 19.5, 30.0, 39.5]
        );
        assert_eq!(lines[0].text, "È luce");
        assert!(lines[1].text.is_empty());
        assert_eq!(lines[3].text, "Fine");
        let lyrics = Lyrics {
            lines,
            plain: String::new(),
            instrumental: false,
            url: String::new(),
        };
        for (pos, index) in [
            (0.0, None),
            (9.75, Some(0)),
            (21.0, Some(1)),
            (60.0, Some(3)),
            (10.0, Some(0)),
            (f64::NAN, None),
        ] {
            assert_eq!(lyrics.active(pos), index);
        }
    }
    #[test]
    fn providers_reject_wrong_artist_version_duration_and_disambiguation() {
        let q = Query {
            artist: "Demo Artist".into(),
            title: "Evening".into(),
            album: "Studio".into(),
            duration: 200,
        };
        let records = serde_json::json!([
            {"artistName":"Demo Artist Jr", "trackName":"Evening","duration":200,"plainLyrics":"sample"},
            {"artistName":"Demo Artist", "trackName":"Evening (Live)","duration":200,"plainLyrics":"sample"},
            {"artistName":"Demo Artist", "trackName":"Evening","duration":240,"plainLyrics":"sample"},
            {"artistName":"Demo Artist", "trackName":"Evening","albumName":"Studio","duration":201,"plainLyrics":"sample"}
        ]);
        assert_eq!(
            lyric_candidate(records.as_array().unwrap(), &q).unwrap()["duration"],
            201
        );
        assert!(lyric_candidate(&records.as_array().unwrap()[..3], &q).is_none());
        let text = "Un musicista immaginario. ".repeat(10);
        let pages = serde_json::json!([{"title":"Demo Artist Jr","extract":text},{"title":"Demo Artist","extract":text,"pageprops":{"disambiguation":""}},{"title":"Demo Artist (musicista)","extract":text}]);
        assert_eq!(
            wiki_candidate(pages.as_array().unwrap(), "Demo Artist").unwrap()["title"],
            "Demo Artist (musicista)"
        );
    }
    #[test]
    fn incomplete_timing_falls_back_to_plain_and_untrusted_lrc_is_bounded() {
        let song = lyrics_record(
            &serde_json::json!({"syncedLyrics":"[00:00]One line","plainLyrics":"Plain text"}),
        );
        assert!(song.lines.is_empty());
        assert_eq!(song.plain, "Plain text");
        let raw = format!("{}Line\n", "[00:00]".repeat(10000)).repeat(200);
        assert_eq!(parse_lrc(&raw).len(), 1); // identical stamps are deduplicated
        let raw = (0..5000)
            .map(|i| format!("[{:02}:{:02}]Line {i}\n", i / 60, i % 60))
            .collect::<String>();
        assert_eq!(parse_lrc(&raw).len(), 4000);
    }
    #[test]
    fn provider_backoff_accepts_both_retry_after_forms_without_early_retry() {
        assert_eq!(retry_delay("120"), Duration::from_secs(300));
        assert_eq!(retry_delay("3600"), Duration::from_secs(3600));
        assert_eq!(retry_delay("invalid"), Duration::from_secs(300));
        let future =
            httpdate::fmt_http_date(std::time::SystemTime::now() + Duration::from_secs(3600));
        let delay = retry_delay(&future).as_secs();
        assert!((3599..=3601).contains(&delay));
    }
    #[test]
    fn no_requests_when_closed_and_stale_responses_cannot_replace_visible_track() {
        let (tx, requests) = mpsc::channel();
        let (responses, rx) = mpsc::channel();
        let mut info = Info {
            tx,
            rx,
            generation: Arc::new(AtomicU64::new(0)),
            query: None,
            language: String::new(),
            artists: HashMap::new(),
            songs: HashMap::new(),
            artist: State::Empty,
            lyrics: State::Empty,
        };
        let a = Query {
            artist: "A".into(),
            title: "One".into(),
            album: "".into(),
            duration: 200,
        };
        let mut b = a.clone();
        b.title = "Two".into();
        info.update(false, Some(a.clone()));
        assert!(requests.try_recv().is_err());
        info.update(true, Some(a.clone()));
        let old = requests.try_recv().unwrap();
        info.update(true, Some(b.clone()));
        let current = requests.try_recv().unwrap();
        responses
            .send(Response {
                epoch: old.epoch,
                query: a,
                payload: Payload::Lyrics(Ok(None)),
            })
            .unwrap();
        info.update(true, Some(b.clone()));
        assert!(matches!(info.lyrics, State::Loading));
        responses
            .send(Response {
                epoch: current.epoch,
                query: b.clone(),
                payload: Payload::Lyrics(Ok(None)),
            })
            .unwrap();
        info.update(true, Some(b.clone()));
        assert!(matches!(info.lyrics, State::Missing));
        info.update(false, None);
        info.update(true, Some(b));
        let reopened = requests.try_recv().unwrap();
        assert!(!reopened.lyrics); // negative cache
    }
    #[test]
    #[ignore = "Read-only public provider integration; requires Internet"]
    fn live_providers_return_artist_and_timed_lyrics() {
        let q = Query {
            artist: "Coldplay".into(),
            title: "Yellow".into(),
            album: "Parachutes".into(),
            duration: 267,
        };
        let mut info = Info::default();
        let deadline = Instant::now() + Duration::from_secs(45);
        info.update(true, Some(q.clone()));
        while (matches!(info.artist, State::Loading) || matches!(info.lyrics, State::Loading))
            && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(50));
            info.update(true, Some(q.clone()));
        }
        let State::Ready(bio) = &info.artist else {
            panic!("Biography unavailable: {:?}", info.artist);
        };
        assert_eq!(bio.title, "Coldplay");
        assert!(bio.url.starts_with("https://it.wikipedia.org/wiki/"));
        let State::Ready(lyrics) = &info.lyrics else {
            panic!("Lyrics unavailable: {:?}", info.lyrics);
        };
        assert!(lyrics.lines.len() > 10);
        println!(
            "Wikipedia title verified; LRCLIB synchronized lines: {}",
            lyrics.lines.len()
        );
        info.update(false, None);
        info.update(true, Some(q));
        assert!(matches!(info.artist, State::Ready(_)));
        assert!(matches!(info.lyrics, State::Ready(_)));
    }
}
