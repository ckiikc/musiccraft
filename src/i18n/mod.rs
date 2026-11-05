//! Per-application catalogs: English keys/fallback, persisted BCP-47 preference.
//! Library metadata, URLs, protocol values and stored paths never pass through this module.
use std::{cell::Cell, collections::HashMap, sync::OnceLock};
pub const LANGUAGES: [(&str, &str); 23] = [
    ("en", "English"),
    ("ja", "日本語"),
    ("zh-hans", "简体中文"),
    ("zh-hant", "繁體中文"),
    ("cs", "Čeština"),
    ("pt-br", "Português (Brasil)"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("fr", "Français"),
    ("ru", "Русский"),
    ("kk", "Қазақша"),
    ("bg", "Български"),
    ("te", "తెలుగు"),
    ("hu", "Magyar"),
    ("uk", "Українська"),
    ("it", "Italiano"),
    ("ar", "العربية"),
    ("pl", "Polski"),
    ("sl", "Slovenščina"),
    ("el", "Ελληνικά"),
    ("tr", "Türkçe"),
    ("lt", "Lietuvių"),
    ("vi", "Tiếng Việt"),
];
thread_local! {static CURRENT:Cell<usize>=const{Cell::new(0)};}
pub fn normalize(value: &str) -> &'static str {
    language_for_locale(value).unwrap_or("en")
}

fn language_for_locale(value: &str) -> Option<&'static str> {
    let tag = value
        .trim()
        .split(['.', '@'])
        .next()?
        .replace('_', "-")
        .to_ascii_lowercase();
    let mut parts = tag.split('-');
    match parts.next()? {
        "zh" => {
            let parts: Vec<_> = parts.collect();
            if parts.contains(&"hans") {
                Some("zh-hans")
            } else if parts
                .iter()
                .any(|part| matches!(*part, "hant" | "tw" | "hk" | "mo"))
            {
                Some("zh-hant")
            } else {
                Some("zh-hans")
            }
        }
        "pt" => Some("pt-br"),
        "c" | "posix" => Some("en"),
        primary => LANGUAGES
            .iter()
            .find_map(|&(code, _)| (code == primary).then_some(code)),
    }
}

pub fn system_language() -> String {
    for name in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(name)
            && let Some(language) = language_for_locale(&value)
        {
            return language.into();
        }
    }
    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleLanguages"])
        .output()
        && output.status.success()
        && let Some(language) = String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| language_for_locale(line.trim().trim_matches(['"', ','])))
    {
        return language.into();
    }
    "en".into()
}
pub fn set(code: &str) {
    let index = LANGUAGES
        .iter()
        .position(|(key, _)| *key == normalize(code))
        .unwrap_or(0);
    CURRENT.with(|c| c.set(index));
}
pub fn language() -> &'static str {
    CURRENT.with(|c| LANGUAGES[c.get()].0)
}
const CATALOG_FILES: [&str; LANGUAGES.len()] = [
    "",
    include_str!("ja.tsv"),
    include_str!("zh-hans.tsv"),
    include_str!("zh-hant.tsv"),
    include_str!("cs.tsv"),
    include_str!("pt-br.tsv"),
    include_str!("de.tsv"),
    include_str!("es.tsv"),
    include_str!("fr.tsv"),
    include_str!("ru.tsv"),
    include_str!("kk.tsv"),
    include_str!("bg.tsv"),
    include_str!("te.tsv"),
    include_str!("hu.tsv"),
    include_str!("uk.tsv"),
    include_str!("it.tsv"),
    include_str!("ar.tsv"),
    include_str!("pl.tsv"),
    include_str!("sl.tsv"),
    include_str!("el.tsv"),
    include_str!("tr.tsv"),
    include_str!("lt.tsv"),
    include_str!("vi.tsv"),
];
fn entries(raw: &'static str) -> impl Iterator<Item = (&'static str, &'static str)> {
    raw.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('\t'))
}

fn catalogs() -> &'static Vec<HashMap<&'static str, &'static str>> {
    static CATALOGS: OnceLock<Vec<HashMap<&'static str, &'static str>>> = OnceLock::new();
    CATALOGS.get_or_init(|| {
        CATALOG_FILES
            .iter()
            .enumerate()
            .map(|(i, raw)| {
                entries(raw)
                    .map(|(key, value)| {
                        let value = if LANGUAGES[i].0 == "ar" {
                            Box::leak(visual_arabic(value).into_boxed_str()) as &str
                        } else {
                            value
                        };
                        (key, value)
                    })
                    .collect()
            })
            .collect()
    })
}
pub fn text(key: &'static str) -> &'static str {
    CURRENT.with(|c| {
        catalogs()[c.get()]
            .get(key)
            .copied()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| key.strip_prefix("Guide: ").unwrap_or(key))
    })
}
/// Search uses the catalog's reading order, before any visual reordering.
pub fn logical_text(key: &'static str) -> &'static str {
    if language() == "ar" {
        static ARABIC: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
        ARABIC
            .get_or_init(|| entries(include_str!("ar.tsv")).collect())
            .get(key)
            .copied()
            .unwrap_or_else(|| text(key))
    } else {
        text(key)
    }
}

/// Format with named placeholders; replacements are opaque user data.
pub fn format(key: &'static str, values: &[(&str, String)]) -> String {
    let template = text(key);
    let mut rest = template;
    let mut result = String::with_capacity(template.len());
    while let Some(open) = rest.find('{') {
        result.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('}').map(|p| p + open) else {
            result.push_str(&rest[open..]);
            return result;
        };
        let name = &rest[open + 1..close];
        if let Some((_, value)) = values.iter().find(|(key, _)| *key == name) {
            result.push_str(value);
        } else {
            result.push_str(&rest[open..=close]);
        }
        rest = &rest[close + 1..];
    }
    result.push_str(rest);
    result
}
/// Localize engine notices at the UI boundary; protocol and device values stay opaque.
pub fn engine_message(message: &str) -> String {
    if let Some(value) = CURRENT.with(|c| catalogs()[c.get()].get(message).copied()) {
        return value.to_string();
    }
    let key = match message {
        "Choose a private local address"
        | "Connect the Mac to a private Wi-Fi or Ethernet network" => Some("Invalid local address"),
        "Player port 3483 is already in use"
        | "Cannot reserve a server port"
        | "Cannot configure the integrated server"
        | "Cannot start the integrated server" => {
            Some("Cannot start server. Check the local address and port.")
        }
        "Bundled server component unavailable" | "Integrated server stopped unexpectedly" => {
            Some("Service unavailable. Try again later.")
        }
        "Integrated server startup timed out" => Some("Connection unavailable or timed out"),
        "Server profile is not writable" => Some("Cache unavailable or not writable"),
        "Scegli un indirizzo privato della rete locale" => Some("Invalid local address"),
        "Nessun brano nella sorgente selezionata" => Some("No tracks found"),
        "Porta server non disponibile" | "Server non disponibile" => {
            Some("Cannot start server. Check the local address and port.")
        }
        "Connessione non disponibile o tempo scaduto"
        | "LMS non raggiungibile o risposta non valida" => {
            Some("Connection unavailable or timed out")
        }
        "Servizio occupato. Riprova più tardi." => Some("Service busy. Try again later."),
        "Risposta del servizio non valida"
        | "Risposta LMS non valida"
        | "Richiesta LMS non valida"
        | "LMS ha rifiutato la richiesta"
        | "Comando LMS non riuscito" => Some("Invalid server response"),
        "Host mancante"
        | "Indirizzo FTP non valido"
        | "Inserisci IP o nome del server FTP"
        | "Server FTP non valido"
        | "URL non valido"
        | "Indirizzo LMS non valido"
        | "Usa l’indirizzo HTTP o HTTPS di LMS senza credenziali nell’URL" => {
            Some("Invalid server address")
        }
        "Porta: usa un numero da 1 a 65535" => Some("Enter a port from 1 to 65535"),
        "Credenziali non valide"
        | "Accesso LMS non valido"
        | "Accesso LMS rifiutato"
        | "Autenticazione non inviata" => Some("Invalid credentials"),
        "Protocollo FTP/FTPS richiesto" => Some("FTP or FTPS is required"),
        "Cache non disponibile"
        | "Cache piena o non scrivibile"
        | "Impossibile creare la cache"
        | "Impossibile finalizzare la cache"
        | "Scrittura cache non riuscita"
        | "Cache non leggibile"
        | "File temporaneo non disponibile" => Some("Cache unavailable or not writable"),
        "File NAS non disponibile"
        | "File non raggiungibile. Verifica che il NAS sia montato"
        | "Lettura NAS interrotta" => Some("File unavailable. Check the NAS connection."),
        "Brano oltre 1 GiB" | "Brano oltre il limite cache di 1 GiB" => {
            Some("Track exceeds the 1 GiB cache limit")
        }
        "Caricamento annullato" | "Salvataggio annullato" | "Salvataggio FTP interrotto" => {
            Some("Operation cancelled")
        }
        "Formato audio non supportato o file danneggiato"
        | "Il decoder ha interrotto il caricamento del file"
        | "Decoder del brano successivo non disponibile" => {
            Some("Unsupported or damaged audio file")
        }
        "Uscita audio non disponibile. Aggiorna l'elenco dei dispositivi" => {
            Some("Audio device unavailable")
        }
        "Formato non accettato dal DAC. Disattiva Segui frequenza sorgente o scegli un'altra uscita" => {
            Some("The device does not support this audio format")
        }
        "Seek non disponibile per questo file" => Some("Seek unavailable for this file"),
        "Impossibile salvare nel Portachiavi: la password resta solo in memoria" => {
            Some("Keychain save failed. The password stays in memory.")
        }
        "Trasferimento interrotto"
        | "Download fallito: verifica indirizzo, accesso e rete"
        | "FTP non raggiungibile: controlla IP, porta, credenziali e percorso"
        | "Trasferimento di rete non disponibile"
        | "Lettura FTP non disponibile"
        | "Controllo FTP non disponibile" => Some("Network transfer failed"),
        "Cartella album non disponibile" => Some("Folder unavailable"),
        "Il dispositivo audio è stato scollegato o ha interrotto lo stream"
        | "Buffer HAL inatteso: uscita esclusiva interrotta" => {
            Some("Output interrupted by the audio device")
        }
        "Il servizio non è disponibile. Riapri la scheda più tardi."
        | "Servizio non disponibile"
        | "Richiesta non disponibile"
        | "LMS non disponibile" => Some("Service unavailable. Try again later."),
        "Copertina non salvabile" => Some("Cannot save the cover"),
        "Percorso di rete non valido" | "Percorso non UTF-8" => Some("Invalid network path"),
        "Gapless: prossimo brano accodato nello stesso stream" => {
            Some("Gapless: next track queued in the same stream")
        }
        "Gapless esclusivo: prossimo brano pronto" => Some("Exclusive gapless: next track ready"),
        "Cambio formato: il prossimo brano richiede la riapertura dell'uscita" => {
            Some("Format change: the output will reopen for the next track")
        }
        _ => None,
    };
    if let Some(key) = key {
        return text(key).to_string();
    }
    for (prefix, key) in [
        (
            "Precaricamento audio non riuscito: ",
            "Audio preload failed: {reason}",
        ),
        (
            "Esclusiva non disponibile: ",
            "Exclusive output unavailable: {reason}",
        ),
    ] {
        if let Some(reason) = message.strip_prefix(prefix) {
            return format(key, &[("reason", engine_message(reason))]);
        }
    }
    message.to_string()
}
pub fn signal_message(message: &str) -> String {
    message
        .replace("Esclusiva ", &std::format!("{} ", text("Exclusive")))
        .replace("Uscita di sistema", text("System output"))
        .replace(
            "rigoroso, 0 dB",
            &std::format!("{} · 0 dB", text("Strict integer")),
        )
        .replace(
            "volume hardware, bit-perfect non certificato",
            text("hardware volume, bit-perfect unverified"),
        )
}

pub fn tracks(n: usize) -> String {
    let one = n == 1;
    let word = match language() {
        "it" => {
            if one {
                "brano"
            } else {
                "brani"
            }
        }
        "fr" => {
            if n <= 1 {
                "morceau"
            } else {
                "morceaux"
            }
        }
        "es" => {
            if one {
                "pista"
            } else {
                "pistas"
            }
        }
        "pt-br" => {
            if one {
                "faixa"
            } else {
                "faixas"
            }
        }
        "de" => "Titel",
        "ja" => "曲",
        "zh-hans" => "首",
        "zh-hant" => "首",
        "ru" => {
            if n % 10 == 1 && n % 100 != 11 {
                "трек"
            } else if (2..=4).contains(&(n % 10)) && !(12..=14).contains(&(n % 100)) {
                "трека"
            } else {
                "треков"
            }
        }
        "uk" => {
            if n % 10 == 1 && n % 100 != 11 {
                "трек"
            } else if (2..=4).contains(&(n % 10)) && !(12..=14).contains(&(n % 100)) {
                "треки"
            } else {
                "треків"
            }
        }
        "kk" => "трек",
        "bg" => {
            if one {
                "песен"
            } else {
                "песни"
            }
        }
        "cs" => {
            if one {
                "skladba"
            } else if (2..=4).contains(&n) {
                "skladby"
            } else {
                "skladeb"
            }
        }
        "pl" => {
            if one {
                "utwór"
            } else if (2..=4).contains(&(n % 10)) && !(12..=14).contains(&(n % 100)) {
                "utwory"
            } else {
                "utworów"
            }
        }
        "sl" => match n % 100 {
            1 => "skladba",
            2 => "skladbi",
            3 | 4 => "skladbe",
            _ => "skladb",
        },
        "ar" => {
            if one {
                "مقطع"
            } else if n == 2 {
                "مقطعان"
            } else if (3..=10).contains(&(n % 100)) {
                "مقاطع"
            } else {
                "مقطعًا"
            }
        }
        "te" => {
            if one {
                "పాట"
            } else {
                "పాటలు"
            }
        }
        "hu" => "szám",
        "tr" => "parça",
        "vi" => "bài hát",
        "el" => {
            if one {
                "κομμάτι"
            } else {
                "κομμάτια"
            }
        }
        "lt" => {
            if n % 10 == 1 && n % 100 != 11 {
                "kūrinys"
            } else if (2..=9).contains(&(n % 10)) && !(11..=19).contains(&(n % 100)) {
                "kūriniai"
            } else {
                "kūrinių"
            }
        }
        _ => {
            if one {
                "track"
            } else {
                "tracks"
            }
        }
    };
    if language() == "ar" {
        visual_arabic(&format!("{n} {word}"))
    } else {
        format!("{n} {word}")
    }
}

// epaint shapes Arabic words. Reorder word runs while leaving each Arabic word
// logical for its shaper. Bidi marks affect display only, never catalog keys/data.
fn visual_arabic(text: &str) -> String {
    // Wrap long help text in logical reading order before visual reordering.
    // Templates stay intact: inserted file names and addresses remain opaque.
    let logical = if text.chars().count() > 85 && !text.contains('{') {
        let mut out = String::new();
        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                out.push('\n');
            }
            let mut count = 0;
            for word in line.split_whitespace() {
                let width = word.chars().count();
                if count > 0 {
                    if count + width + 1 > 55 {
                        out.push('\n');
                        count = 0;
                    } else {
                        out.push(' ');
                        count += 1;
                    }
                }
                out.push_str(word);
                count += width;
            }
        }
        out
    } else {
        text.to_string()
    };
    // Protect formatting tokens and keyboard shortcuts as indivisible LTR atoms.
    let mut atoms = Vec::new();
    let mut masked = String::new();
    let mut at = 0;
    while at < logical.len() {
        let tail = &logical[at..];
        let c = tail.chars().next().unwrap();
        let length = if c == '{' {
            tail.find('}').map(|end| end + 1)
        } else if c == '⌘' {
            tail[c.len_utf8()..]
                .chars()
                .next()
                .map(|next| c.len_utf8() + next.len_utf8())
        } else {
            None
        };
        if let Some(length) = length.filter(|_| atoms.len() < 128) {
            atoms.push(&logical[at..at + length]);
            masked.push(char::from_u32(0xe000 + atoms.len() as u32 - 1).unwrap());
            at += length;
        } else {
            masked.push(c);
            at += c.len_utf8();
        }
    }
    let info = unicode_bidi::BidiInfo::new(&masked, None);
    let mut result = String::new();
    for paragraph in &info.paragraphs {
        let raw = &masked[paragraph.range.clone()];
        let content = raw.trim_end_matches(['\r', '\n']);
        let visual = info.reorder_line(
            paragraph,
            paragraph.range.start..paragraph.range.start + content.len(),
        );
        let mut word = String::new();
        let flush = |word: &mut String, result: &mut String| {
            result.extend(word.chars().rev());
            word.clear();
        };
        for c in visual.chars() {
            if matches!(c,'\u{0620}'..='\u{065f}'|'\u{0670}'..='\u{06d3}'|'\u{06d5}'..='\u{06ed}') {
                word.push(c);
            } else {
                flush(&mut word, &mut result);
                if let Some(atom) = u32::from(c)
                    .checked_sub(0xe000)
                    .and_then(|i| atoms.get(i as usize))
                {
                    result.push_str(atom);
                } else {
                    result.push(c);
                }
            }
        }
        flush(&mut word, &mut result);
        if content.len() < raw.len() {
            result.push('\n');
        }
    }
    result
}

#[macro_export]
macro_rules! tr {
    ($key:literal) => {
        $crate::i18n::text($key)
    };
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arabic_visual_lines_keep_separators_at_the_end() {
        let short = "مكتبة الموسيقى\nدليل الاستخدام";
        let visual = visual_arabic(short);
        assert_eq!(visual.lines().count(), 2);
        assert!(!visual.starts_with('\n'));
        let long = format!(
            "{}\n{}",
            "دليل الموسيقى باللغة العربية ".repeat(5),
            "إعدادات التشغيل ".repeat(5)
        );
        let visual = visual_arabic(&long);
        assert!(visual.lines().count() > 2);
        assert!(!visual.starts_with('\n'));
        assert!(!visual.contains("\n\n"));
        let tokens = visual_arabic("الخادم {address} الإصدار 9.1.1\nالمساعدة F1");
        assert!(tokens.contains("{address}"));
        assert!(tokens.contains("9.1.1"));
        assert!(tokens.contains("F1"));
    }
    #[test]
    fn all_pdfcraft_languages_and_locale_aliases_are_supported() {
        assert_eq!(
            LANGUAGES.map(|(code, _)| code),
            [
                "en", "ja", "zh-hans", "zh-hant", "cs", "pt-br", "de", "es", "fr", "ru", "kk",
                "bg", "te", "hu", "uk", "it", "ar", "pl", "sl", "el", "tr", "lt", "vi"
            ]
        );
        assert_eq!(normalize("vi_VN.UTF-8"), "vi");
        assert_eq!(normalize("zh-MO"), "zh-hant");
        assert_eq!(normalize("zh-Hans-HK"), "zh-hans");
        assert_eq!(normalize("zh_TW"), "zh-hant");
        assert_eq!(normalize("pt-PT"), "pt-br");
        assert_eq!(normalize("it_IT.UTF-8"), "it");
        assert_eq!(normalize("xx"), "en");
        set("it");
        assert_eq!(text("Library"), "Libreria");
        assert_eq!(text("untranslated sentinel"), "untranslated sentinel");
        set("en");
    }
    #[test]
    fn placeholder_values_remain_opaque() {
        set("en");
        assert_eq!(
            format(
                "Cover · {provider}",
                &[("provider", "Eric {artist}".into())]
            ),
            "Cover · Eric {artist}"
        );
    }
    fn placeholders(text: &str) -> std::collections::BTreeSet<&str> {
        let mut result = std::collections::BTreeSet::new();
        let mut rest = text;
        while let Some(start) = rest.find('{') {
            let Some(end) = rest[start + 1..].find('}') else {
                break;
            };
            result.insert(&rest[start + 1..start + 1 + end]);
            rest = &rest[start + end + 2..];
        }
        result
    }
    #[test]
    fn every_catalog_has_every_key_and_preserves_placeholders() {
        let all = catalogs();
        let reference = &all[1];
        assert!(!reference.is_empty());
        for (&(code, _), raw) in LANGUAGES.iter().zip(CATALOG_FILES).skip(1) {
            let mut seen = std::collections::HashSet::new();
            for row in raw
                .lines()
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
            {
                let (key, value) = row
                    .split_once('\t')
                    .expect("catalog row must contain a tab");
                assert!(
                    !key.trim().is_empty() && !value.contains('\t'),
                    "{code}: invalid row {row}"
                );
                assert!(seen.insert(key), "{code}: duplicate key {key}");
            }
        }
        for (index, catalog) in all.iter().enumerate().skip(1) {
            assert_eq!(
                catalog.len(),
                reference.len(),
                "{} coverage",
                LANGUAGES[index].0
            );
            for &key in reference.keys() {
                let value = catalog
                    .get(key)
                    .unwrap_or_else(|| panic!("{} missing {key}", LANGUAGES[index].0));
                assert!(
                    !value.trim().is_empty(),
                    "{} empty {key}",
                    LANGUAGES[index].0
                );
                assert_eq!(
                    placeholders(key),
                    placeholders(value),
                    "{} placeholders: {key} => {value}",
                    LANGUAGES[index].0
                );
            }
        }
    }
    #[test]
    fn all_ui_translation_keys_exist_and_metadata_replacements_are_not_recursive() {
        let keys = &catalogs()[1];
        for source in [
            include_str!("../app.rs"),
            include_str!("../app/miniplayer.rs"),
            include_str!("../app/desktop.rs"),
            include_str!("../app/settings.rs"),
            include_str!("../app/help.rs"),
            include_str!("../app/browser.rs"),
            include_str!("../app/listening_ui.rs"),
            include_str!("../facets.rs"),
        ] {
            let end = source
                .rfind("\n#[cfg(test)]\nmod tests")
                .unwrap_or(source.len());
            let mut remaining = &source[..end];
            while let Some(start) = remaining.find("tr!(\"") {
                remaining = &remaining[start + 5..];
                let end = remaining.find('"').unwrap();
                let key = &remaining[..end];
                assert!(keys.contains_key(key), "missing UI key {key}");
                remaining = &remaining[end + 1..];
            }
        }
        set("en");
        assert_eq!(
            format(
                "{source} found · connection updated to {address}",
                &[
                    ("source", "{address}".into()),
                    ("address", "10.0.0.2".into())
                ]
            ),
            "{address} found · connection updated to 10.0.0.2"
        );
        set("ar");
        let text = format(
            "Server active · {count} · {address}",
            &[("count", "12".into()), ("address", "10.0.0.2:9042".into())],
        );
        assert!(text.contains("10.0.0.2:9042"));
        assert!(!text.contains('{'));
        set("en");
    }
    #[test]
    fn count_forms_follow_locale_rules_without_changing_library_data() {
        set("ru");
        assert_eq!(tracks(1), "1 трек");
        assert_eq!(tracks(12), "12 треков");
        assert_eq!(tracks(22), "22 трека");
        set("sl");
        assert_eq!(tracks(2), "2 skladbi");
        set("it");
        assert_eq!(tracks(1), "1 brano");
        assert_eq!(tracks(0), "0 brani");
        set("vi");
        assert_eq!(tracks(1), "1 bài hát");
        assert_eq!(tracks(25), "25 bài hát");
        set("en");
    }
}
