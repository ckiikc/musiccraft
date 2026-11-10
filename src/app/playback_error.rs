//! Keep a failed track identifiable without pretending it is still playing.
use super::*;

pub(super) struct Failure {
    source: Option<String>,
    reason: String,
}

impl Aurea {
    pub(super) fn displayed_track(&self) -> Option<&Track> {
        self.requested
            .as_ref()
            .or(self.current.as_ref())
            .or_else(|| self.playback_error.as_ref()?.source.as_ref())
            .and_then(|id| self.saved.tracks.iter().find(|t| &t.source == id))
    }

    pub(super) fn playback_message(&self) -> String {
        if let Some(failure) = &self.playback_error {
            let reason = crate::i18n::engine_message(&failure.reason);
            if let Some(track) = self.displayed_track() {
                return format!("{} — {}: {reason}", track.artist, track.title);
            }
            return reason;
        }
        crate::i18n::engine_message(&self.message)
    }

    pub(super) fn playback_failed(&mut self, source: Option<String>, reason: String) {
        // A decoder or output failure says nothing about the phone's reachability.
        // Remote reads retain this marker even when wrapped by the demuxer.
        if reason.contains("Lettura audio dalla rete: ")
            && let Some(endpoint) = source
                .as_deref()
                .and_then(|id| self.source_for_track(id))
                .cloned()
        {
            self.connections.invalidate(&endpoint);
        }
        self.stop();
        self.output = tr!("Output closed after an error").into();
        self.history.cancel();
        self.message = reason.clone();
        self.playback_error = Some(Failure { source, reason });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_next_track_preserves_identity_and_phone_readiness() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = Aurea::new(&cc, dir.path().into(), vec![], None, None);
        let (commands, _receiver) = mpsc::channel();
        let (events, receiver) = mpsc::channel();
        app.audio.tx = commands;
        app.audio.rx = receiver;
        app.saved.auto_sync = false;
        app.network_probe_pending = true;
        let phone = crate::phone::pairing(&format!(
            "musiccraft://pair?v=1&host=127.0.0.1&port=8737&id=01234567-89ab-cdef-0123-456789abcdef&pin={}=&token={}",
            "A".repeat(43), "B".repeat(43)
        )).unwrap();
        let root = phone.url().unwrap();
        app.saved.active_source = Some(phone.id.clone());
        app.saved.sources = vec![phone.clone()];
        app.connections.mark_verified(&phone);
        app.saved.tracks = ["previous.flac", "failed.flac"]
            .map(|name| Track::placeholder(format!("{root}{name}")))
            .into();
        app.saved.tracks[1].title = "Identifiable failed song".into();
        app.saved.tracks[1].artist = "Synthetic artist".into();
        app.current = Some(app.saved.tracks[0].source.clone());
        let failed = app.saved.tracks[1].source.clone();
        app.planned = Some(failed.clone());
        let ticket = app.ticket;
        events
            .send(Event::TrackError {
                ticket: ticket.wrapping_add(1),
                source: failed.clone(),
                reason: "obsolete".into(),
            })
            .unwrap();
        app.poll(&ctx);
        assert!(app.playback_error.is_none());
        events
            .send(Event::TrackError {
                ticket,
                source: failed.clone(),
                reason: "Errore di decodifica audio: malformed frame".into(),
            })
            .unwrap();
        app.poll(&ctx);
        assert!(app.navigation_ready());
        assert!(app.current.is_none() && app.requested.is_none() && app.paused);
        assert_eq!(app.displayed_track().unwrap().source, failed);
        assert!(app.playback_message().contains("Identifiable failed song"));
        assert!(app.playback_message().contains("malformed frame"));
        // The output can drain its queue just after the decoder reports failure.
        // Those late notifications must not erase the error or restart a song.
        events.send(Event::Position(12.0, false)).unwrap();
        events.send(Event::Ended).unwrap();
        app.poll(&ctx);
        assert!(app.paused && app.current.is_none() && app.requested.is_none());
        assert!(app.playback_error.is_some());
        // A device error likewise must not take the phone offline.
        app.playback_failed(Some(failed.clone()), "Audio device unavailable".into());
        assert!(app.track_connection_ready(&failed));
        app.playback_failed(
            Some(failed.clone()),
            "Lettura PCM interrotta: Lettura audio dalla rete: connection reset".into(),
        );
        assert!(!app.track_connection_ready(&failed));
        app.stop();
        assert!(app.playback_error.is_none());
    }
}
