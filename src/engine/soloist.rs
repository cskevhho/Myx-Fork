//! Official Soloist transport. All subprocess and HTTP work stays off the UI thread.
use super::EngineEvent;
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) struct Engine {
    tx: flume::Sender<Request>,
}

enum Request {
    Control(Vec<Vec<String>>),
    Tracks {
        tracks: Vec<String>,
        start: Option<String>,
        position: u32,
        shuffle: bool,
    },
    Toggle,
}

impl Engine {
    pub fn start(events: flume::Sender<EngineEvent>) -> Result<Self> {
        let binary = std::env::var("MYX_SOLOIST_BIN").unwrap_or_else(|_| "soloist".into());
        Self::start_with_binary(events, binary)
    }

    fn start_with_binary(events: flume::Sender<EngineEvent>, binary: String) -> Result<Self> {
        run(&binary, &["--version".into()]).context("install Soloist or set MYX_SOLOIST_BIN")?;
        let (tx, rx) = flume::bounded::<Request>(16);
        std::thread::Builder::new().name("soloist".into()).spawn(move || {
            let mut state = Snapshot::default();
            let mut last_error = String::new();
            let mut last_poll = Instant::now() - Duration::from_secs(2);
            loop {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(request) => {
                        let result = match request {
                            Request::Control(commands) => commands.iter().try_for_each(|args| ctl(&binary, args).map(|_| ())),
                            Request::Tracks { tracks, start, position, shuffle } => play_tracks(&binary, tracks, start, position, shuffle),
                            Request::Toggle => ctl(&binary, &[if state.playing { "pause" } else { "play" }.into()]).map(|_| ()),
                        };
                        if let Err(e) = result {
                            let _ = events.send(EngineEvent::Failed { message: format!("Soloist: {e:#}") });
                        }
                        last_poll = Instant::now() - Duration::from_secs(2);
                    }
                    Err(flume::RecvTimeoutError::Disconnected) => break,
                    Err(flume::RecvTimeoutError::Timeout) => {}
                }
                if last_poll.elapsed() < Duration::from_secs(1) { continue; }
                last_poll = Instant::now();
                match ctl(&binary, &["now".into(), "--json".into()])
                    .and_then(|s| serde_json::from_str::<Value>(&s).context("invalid playback state")) {
                    Ok(value) => {
                        last_error.clear();
                        for event in state.update(&value) {
                            if events.send(event).is_err() { return; }
                        }
                    }
                    Err(e) => {
                        let message = format!("Soloist: {e:#}. Start Soloist and select it in Spotify's device picker.");
                        if message != last_error {
                            let _ = events.send(EngineEvent::Failed { message: message.clone() });
                            last_error = message;
                        }
                    }
                }
            }
        }).context("start Soloist control worker")?;
        Ok(Self { tx })
    }

    fn submit(&self, request: Request) -> Result<()> {
        self.tx
            .try_send(request)
            .map_err(|_| anyhow!("Soloist is busy or disconnected; try again shortly"))
    }
    fn controls(&self, commands: Vec<Vec<String>>) -> Result<()> {
        self.submit(Request::Control(commands))
    }
    fn control(&self, args: &[&str]) -> Result<()> {
        self.controls(vec![args.iter().map(|s| s.to_string()).collect()])
    }
    pub fn play_context(&self, uri: String, shuffle: bool) -> Result<()> {
        self.play_context_at(uri, None, 0, shuffle)
    }
    pub fn play_context_at(
        &self,
        uri: String,
        track: Option<String>,
        position: u32,
        shuffle: bool,
    ) -> Result<()> {
        // Offsets must retain the context; the Web API supports that atomically.
        if track.is_some() || position > 0 {
            return self.submit(Request::Tracks {
                tracks: vec![uri],
                start: track,
                position,
                shuffle,
            });
        }
        self.controls(vec![
            vec!["shuffle".into(), if shuffle { "on" } else { "off" }.into()],
            vec!["play".into(), uri],
        ])
    }
    pub fn play_tracks(
        &self,
        tracks: Vec<String>,
        start: Option<String>,
        position: u32,
        shuffle: bool,
    ) -> Result<()> {
        if tracks.is_empty() {
            bail!("no tracks to play");
        }
        if tracks.len() > 100 {
            bail!("Soloist supports up to 100 explicit tracks; play a playlist or album instead");
        }
        self.submit(Request::Tracks {
            tracks,
            start,
            position,
            shuffle,
        })
    }
    pub fn play_track_at(&self, uri: String, position: u32) -> Result<()> {
        self.play_tracks(vec![uri.clone()], Some(uri), position, false)
    }
    pub fn play(&self) -> Result<()> {
        self.control(&["play"])
    }
    pub fn pause(&self) -> Result<()> {
        self.control(&["pause"])
    }
    pub fn next(&self) -> Result<()> {
        self.control(&["next"])
    }
    pub fn prev(&self) -> Result<()> {
        self.control(&["prev"])
    }
    pub fn toggle(&self) -> Result<()> {
        self.submit(Request::Toggle)
    }
    pub fn shuffle(&self, on: bool) -> Result<()> {
        self.control(&["shuffle", if on { "on" } else { "off" }])
    }
    pub fn repeat(&self, on: bool) -> Result<()> {
        self.control(&["repeat", if on { "context" } else { "off" }])
    }
    pub fn set_volume(&self, vol: u16) -> Result<()> {
        self.control(&["volume", &(u32::from(vol) * 100 / 65535).to_string()])
    }
    pub fn seek(&self, position_ms: u32) -> Result<()> {
        self.control(&["seek", &position_ms.to_string()])
    }
}

fn ctl(binary: &str, args: &[String]) -> Result<String> {
    let mut full = vec!["ctl".into()];
    if let Ok(dir) = std::env::var("MYX_SOLOIST_DATA_DIR") {
        full.extend(["--data-dir".into(), dir]);
    }
    full.extend_from_slice(args);
    run(binary, &full)
}

fn run(binary: &str, args: &[String]) -> Result<String> {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("launch Soloist")?;
    // Drain both pipes concurrently: large playback snapshots must not fill a pipe
    // and make a healthy daemon look like a timeout.
    let stdout = child.stdout.take().context("Soloist stdout")?;
    let stderr = child.stderr.take().context("Soloist stderr")?;
    let out = std::thread::spawn(move || {
        use std::io::Read;
        let mut b = String::new();
        let _ = stdout.take(1_048_576).read_to_string(&mut b);
        b
    });
    let err = std::thread::spawn(move || {
        use std::io::Read;
        let mut b = String::new();
        let _ = stderr.take(65_536).read_to_string(&mut b);
        b
    });
    let deadline = Instant::now() + Duration::from_secs(4);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    let Some(status) = status else {
        bail!("local control timed out");
    };
    if !status.success() {
        bail!("{}", stderr.trim().chars().take(250).collect::<String>());
    }
    Ok(stdout)
}

fn play_tracks(
    binary: &str,
    tracks: Vec<String>,
    start: Option<String>,
    position: u32,
    shuffle: bool,
) -> Result<()> {
    let mut web = crate::webapi::WebApi::init()?;
    let token = web.valid_token()?;
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()?;
    let response = client
        .get("https://api.spotify.com/v1/me/player/devices")
        .bearer_auth(&token)
        .send()?
        .error_for_status()?;
    let devices: Value = response.json()?;
    let name = std::env::var("MYX_SOLOIST_DEVICE").unwrap_or_else(|_| "Myx (Soloist)".into());
    let id = devices["devices"]
        .as_array()
        .and_then(|ds| ds.iter().find(|d| d["name"].as_str() == Some(&name)))
        .and_then(|d| d["id"].as_str())
        .context("Soloist device is not paired; select Myx (Soloist) in Spotify first")?;
    let body = playback_request(&tracks, start.as_deref(), position)?;
    // Set shuffle on the named device, without stealing another device's playback.
    ctl(
        binary,
        &["shuffle".into(), if shuffle { "on" } else { "off" }.into()],
    )?;
    client
        .put(format!(
            "https://api.spotify.com/v1/me/player/play?device_id={}",
            crate::util::urlencode(id)
        ))
        .bearer_auth(&token)
        .json(&body)
        .send()?
        .error_for_status()?;
    Ok(())
}

fn playback_request(tracks: &[String], start: Option<&str>, position: u32) -> Result<Value> {
    let mut body = json!({"position_ms":position});
    let context = tracks.len() == 1 && !tracks[0].starts_with("spotify:track:");
    if context {
        body["context_uri"] = json!(tracks[0]);
        if let Some(uri) = start {
            body["offset"] = json!({"uri":uri});
        }
    } else {
        if tracks.is_empty() || tracks.len() > 100 {
            bail!("explicit playback needs 1–100 tracks");
        }
        body["uris"] = json!(tracks);
        if let Some(uri) = start {
            let index = tracks
                .iter()
                .position(|s| s == uri)
                .context("selected track is not in the queue")?;
            body["offset"] = json!({"position":index});
        }
    }
    Ok(body)
}

#[derive(Default)]
struct Snapshot {
    uri: String,
    playing: bool,
    status: String,
}
impl Snapshot {
    fn update(&mut self, v: &Value) -> Vec<EngineEvent> {
        let mut events = Vec::new();
        let uri = v["item"]["uri"].as_str().unwrap_or("");
        let status = v["status"].as_str().unwrap_or("idle");
        let position = v["position"]["position_ms"]
            .as_u64()
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32;
        if uri != self.uri && !uri.is_empty() {
            events.push(EngineEvent::TrackChanged { uri: uri.into() });
        }
        if status == "idle" {
            if self.status != "idle" {
                events.push(EngineEvent::Stopped);
            }
        } else if status == "playing" || status == "paused" {
            let changed = self.uri != uri || self.status != status;
            if changed {
                events.push(if status == "playing" {
                    EngineEvent::Playing {
                        uri: uri.into(),
                        position_ms: position,
                    }
                } else {
                    EngineEvent::Paused {
                        uri: uri.into(),
                        position_ms: position,
                    }
                });
            } else {
                events.push(EngineEvent::PositionCorrection {
                    uri: uri.into(),
                    position_ms: position,
                });
            }
        }
        self.uri = uri.into();
        self.status = status.into();
        self.playing = status == "playing";
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn official_playback_snapshots_drive_track_play_pause_and_stop() {
        let mut s = Snapshot::default();
        let mut v = json!({"status":"playing","item":{"uri":"spotify:track:a"},"position":{"position_ms":1000}});
        let e = s.update(&v);
        assert!(matches!(
            &e[..],
            [
                EngineEvent::TrackChanged { .. },
                EngineEvent::Playing {
                    position_ms: 1000,
                    ..
                }
            ]
        ));
        assert!(matches!(
            &s.update(&v)[..],
            [EngineEvent::PositionCorrection { .. }]
        ));
        v["status"] = json!("paused");
        assert!(matches!(&s.update(&v)[..], [EngineEvent::Paused { .. }]));
        v["status"] = json!("idle");
        assert!(matches!(&s.update(&v)[..], [EngineEvent::Stopped]));
        assert!(s.update(&v).is_empty());
    }
    #[test]
    fn context_resume_retains_playlist_and_explicit_tracks_use_position_offsets() {
        let b = playback_request(
            &["spotify:playlist:p".into()],
            Some("spotify:track:b"),
            4200,
        )
        .unwrap();
        assert_eq!(
            b,
            json!({"context_uri":"spotify:playlist:p","offset":{"uri":"spotify:track:b"},"position_ms":4200})
        );
        let b = playback_request(
            &["spotify:track:a".into(), "spotify:track:b".into()],
            Some("spotify:track:b"),
            0,
        )
        .unwrap();
        assert_eq!(b["offset"], json!({"position":1}));
        assert!(playback_request(&[], None, 0).is_err());
        assert!(playback_request(
            &["spotify:track:a".into()],
            Some("spotify:track:missing"),
            0
        )
        .is_err());
    }
}
