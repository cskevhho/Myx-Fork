//! Playback facade: native librespot, or the official Soloist receiver.
pub mod auth;
mod native;
mod soloist;

use crate::audio::{EqualizerSettings, VisBands};
use anyhow::{bail, Result};
use librespot_core::{authentication::Credentials, Session};
use std::sync::{Arc, Mutex};

/// A normalized playback event surfaced to the rest of the app.
#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// A new track became current — carries its Spotify URI. This is the reactive
    /// theme trigger.
    TrackChanged {
        uri: String,
    },
    Playing {
        uri: String,
        position_ms: u32,
    },
    Paused {
        uri: String,
        position_ms: u32,
    },
    Stopped,
    /// A playback request failed; never leave the UI stuck on starting.
    Failed {
        message: String,
    },
    /// The access point went away and a replacement connection is being built.
    Reconnecting,
    /// Playback control works again. Whatever was playing is not resumed — the
    /// new Connect device starts idle.
    Reconnected,
    EndOfTrack {
        uri: String,
    },
    PositionCorrection {
        uri: String,
        position_ms: u32,
    },
}

pub struct Engine {
    pub bands: Arc<Mutex<VisBands>>,
    backend: Backend,
}
enum Backend {
    Native(native::Engine),
    Soloist(soloist::Engine),
}

pub fn uses_soloist() -> bool {
    std::env::var("MYX_BACKEND").as_deref() == Ok("soloist")
}

pub fn needs_authorization() -> bool {
    !uses_soloist() && native::needs_authorization()
}
pub fn credentials() -> Result<Credentials> {
    if uses_soloist() {
        // Soloist handles its own authentication, not librespot's OAuth flow.
        Ok(Credentials::with_access_token(String::new()))
    } else {
        native::credentials()
    }
}

pub async fn run(creds: Credentials, tx: flume::Sender<EngineEvent>, volume: u8) -> Result<Engine> {
    if uses_soloist() {
        let remote = soloist::Engine::start(tx)?;
        Ok(Engine {
            bands: VisBands::shared(),
            backend: Backend::Soloist(remote),
        })
    } else {
        let native = native::run(creds, tx, volume).await?;
        Ok(Engine {
            bands: native.bands.clone(),
            backend: Backend::Native(native),
        })
    }
}

impl Engine {
    pub async fn web_token(&self) -> Result<String> {
        match &self.backend {
            Backend::Native(e) => e.web_token().await,
            Backend::Soloist(_) => {
                tokio::task::spawn_blocking(|| {
                    let mut web = crate::webapi::WebApi::init()?;
                    web.valid_token()
                })
                .await?
            }
        }
    }

    pub fn supports_equalizer(&self) -> bool {
        matches!(self.backend, Backend::Native(_))
    }
    pub fn session(&self) -> Option<Session> {
        match &self.backend {
            Backend::Native(e) => Some(e.session()),
            Backend::Soloist(_) => None,
        }
    }
    pub fn set_equalizer(&self, settings: EqualizerSettings) {
        if let Backend::Native(e) = &self.backend {
            e.set_equalizer(settings);
        }
    }
    pub fn stop(&self) {
        match &self.backend {
            Backend::Native(e) => e.stop(),
            Backend::Soloist(e) => {
                let _ = e.pause();
            }
        }
    }
    pub fn play_context(&self, context_uri: impl Into<String>, shuffle: bool) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.play_context(context_uri.into(), shuffle),
            Backend::Soloist(e) => e.play_context(context_uri.into(), shuffle),
        }
    }
    pub fn play_context_at(
        &self,
        context_uri: String,
        track_uri: Option<String>,
        position_ms: u32,
        shuffle: bool,
    ) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.play_context_at(context_uri, track_uri, position_ms, shuffle),
            Backend::Soloist(e) => e.play_context_at(context_uri, track_uri, position_ms, shuffle),
        }
    }
    pub fn play_tracks(
        &self,
        tracks: Vec<String>,
        start_uri: Option<String>,
        start_position_ms: u32,
        shuffle: bool,
    ) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.play_tracks(tracks, start_uri, start_position_ms, shuffle),
            Backend::Soloist(e) => e.play_tracks(tracks, start_uri, start_position_ms, shuffle),
        }
    }
    pub fn play_track_at(&self, uri: String, position_ms: u32) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.play_track_at(uri, position_ms),
            Backend::Soloist(e) => e.play_track_at(uri, position_ms),
        }
    }
    pub fn play(&self) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.play(),
            Backend::Soloist(e) => e.play(),
        }
    }
    pub fn pause(&self) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.pause(),
            Backend::Soloist(e) => e.pause(),
        }
    }
    pub fn toggle(&self) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.toggle(),
            Backend::Soloist(e) => e.toggle(),
        }
    }
    pub fn next(&self) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.next(),
            Backend::Soloist(e) => e.next(),
        }
    }
    pub fn prev(&self) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.prev(),
            Backend::Soloist(e) => e.prev(),
        }
    }
    pub fn shuffle(&self, on: bool) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.shuffle(on),
            Backend::Soloist(e) => e.shuffle(on),
        }
    }
    pub fn repeat(&self, on: bool) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.repeat(on),
            Backend::Soloist(e) => e.repeat(on),
        }
    }
    pub fn set_volume(&self, vol: u16) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.set_volume(vol),
            Backend::Soloist(e) => e.set_volume(vol),
        }
    }
    pub fn seek(&self, position_ms: u32) -> Result<()> {
        match &self.backend {
            Backend::Native(e) => e.seek(position_ms),
            Backend::Soloist(e) => e.seek(position_ms),
        }
    }
}

pub async fn radio_tracks(session: &Option<Session>, uri: &str) -> Result<Vec<String>> {
    let Some(session) = session else {
        bail!("Radio is unavailable with Soloist; play an album or playlist instead");
    };
    native::radio_tracks(session, uri).await
}
