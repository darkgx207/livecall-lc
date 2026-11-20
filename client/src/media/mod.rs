//! WebRTC P2P signaling and Opus audio for LiveCall (subphase 4B).
mod audio;
use std::{collections::HashMap, sync::Arc, time::Duration};
use bytes::Bytes;
use audio::AudioRuntime;
use serde_json::{json, Value};
use tokio::sync::{mpsc::UnboundedSender, Mutex};
use webrtc::{
    api::{media_engine::MediaEngine as WebRtcMediaEngine, API, APIBuilder},
    media::Sample,
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::{track_local_static_sample::TrackLocalStaticSample, TrackLocal},
    ice_transport::{ice_candidate::RTCIceCandidateInit, ice_server::RTCIceServer},
    peer_connection::{
        configuration::RTCConfiguration,
        sdp::session_description::RTCSessionDescription,
        RTCPeerConnection,
    },
};

#[derive(Debug, Clone)]
pub struct MediaConfig {
    pub ice_servers: Vec<RTCIceServer>,
    pub max_peers: usize,
}
impl Default for MediaConfig {
    fn default() -> Self {
        let urls = std::env::var("LIVECALL_STUN_URLS")
            .or_else(|_| std::env::var("LIVECALL_STUN_URL"))
            .unwrap_or_else(|_| "stun:stun.l.google.com:19302".to_string());
        let mut ice_servers: Vec<RTCIceServer> = urls.split(',').map(str::trim).filter(|s| !s.is_empty())
            .map(|url| RTCIceServer { urls: vec![url.to_string()], ..Default::default() }).collect();
        if let Ok(turn_url) = std::env::var("LIVECALL_TURN_URL") {
            if !turn_url.trim().is_empty() {
                ice_servers.push(RTCIceServer {
                    urls: vec![turn_url],
                    username: std::env::var("LIVECALL_TURN_USERNAME").unwrap_or_default(),
                    credential: std::env::var("LIVECALL_TURN_PASSWORD").unwrap_or_default(),
                    ..Default::default()
                });
            }
        }
        Self { ice_servers, max_peers: 6 }
    }
}

#[derive(Clone)]
pub struct MediaEngine {
    api: Arc<API>,
    peers: Arc<Mutex<HashMap<String, Arc<RTCPeerConnection>>>>,
    pending_ice: Arc<Mutex<HashMap<String, Vec<RTCIceCandidateInit>>>>,
    outbound: UnboundedSender<String>,
    max_peers: usize,
    ice_servers: Vec<RTCIceServer>,
    audio: Option<Arc<AudioRuntime>>,
    audio_track: Option<Arc<TrackLocalStaticSample>>,
    audio_error: Option<String>,
}

impl std::fmt::Debug for MediaEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MediaEngine").field("max_peers", &self.max_peers).finish_non_exhaustive()
    }
}

impl MediaEngine {
    pub async fn new(config: MediaConfig, outbound: UnboundedSender<String>) -> Result<Self, String> {
        let mut codecs = WebRtcMediaEngine::default();
        codecs.register_default_codecs().map_err(|e| format!("registro de codecs WebRTC: {e}"))?;
        let api = APIBuilder::new().with_media_engine(codecs).build();
        let (audio, audio_track, audio_error) = match AudioRuntime::start() {
            Ok((runtime, mut samples)) => {
                let track = Arc::new(TrackLocalStaticSample::new(
                    RTCRtpCodecCapability { mime_type: "audio/opus".into(), clock_rate: 48_000, channels: 2, sdp_fmtp_line: "minptime=10;useinbandfec=1".into(), rtcp_feedback: vec![] },
                    "livecall-mic".into(), "livecall-audio".into(),
                ));
                let writer = track.clone();
                tokio::spawn(async move {
                    while let Some(data) = samples.recv().await {
                        // A shared track fans out the same encoded microphone frame to all peers.
                        if let Err(error) = writer.write_sample(&Sample {
                            data: Bytes::from(data), duration: Duration::from_millis(20), ..Default::default()
                        }).await { eprintln!("[Áudio] envio RTP: {error}"); }
                    }
                });
                (Some(Arc::new(runtime)), Some(track), None)
            }
            Err(error) => {
                eprintln!("[Áudio] dispositivos indisponíveis: {error}");
                (None, None, Some(error))
            }
        };
        Ok(Self { api: Arc::new(api), peers: Arc::new(Mutex::new(HashMap::new())), pending_ice: Arc::new(Mutex::new(HashMap::new())), outbound, max_peers: config.max_peers, ice_servers: config.ice_servers, audio, audio_track, audio_error })
    }

    pub fn audio_error(&self) -> Option<&str> { self.audio_error.as_deref() }
    pub fn set_deafened(&self, enabled: bool) {
        if let Some(audio) = &self.audio { audio.playback.set_deafened(enabled); }
    }
    pub fn set_microphone_enabled(&self, enabled: bool) {
        if let Some(audio) = &self.audio { audio.set_muted(!enabled); }
    }
    /// Local-only per-peer volume; independent of microphone mute and deafen.
    pub fn set_peer_volume(&self, peer_id: &str, percent: u8) {
        if let Some(audio) = &self.audio {
            audio.playback.set_peer_gain(peer_id, f32::from(percent.min(100)) / 100.0);
        }
    }

    async fn peer(&self, peer_id: &str) -> Result<Arc<RTCPeerConnection>, String> {
        if let Some(pc) = self.peers.lock().await.get(peer_id).cloned() { return Ok(pc); }
        {
            let peers = self.peers.lock().await;
            if peers.len() >= self.max_peers { return Err(format!("limite P2P de {} participantes remotos atingido", self.max_peers)); }
        }
        let pc = Arc::new(self.api.new_peer_connection(RTCConfiguration { ice_servers: self.ice_servers.clone(), ..Default::default() }).await.map_err(|e| e.to_string())?);
        let tx = self.outbound.clone();
        let target = peer_id.to_string();
        pc.on_ice_candidate(Box::new(move |candidate| {
            let tx = tx.clone(); let target = target.clone();
            Box::pin(async move {
                if let Some(candidate) = candidate {
                    if let Ok(init) = candidate.to_json() {
                        let payload = json!({"kind":"ice_candidate", "candidate":init.candidate, "sdp_mid":init.sdp_mid, "sdp_mline_index":init.sdp_mline_index});
                        let _ = send_signal(&tx, &target, payload);
                    }
                }
            })
        }));
        let state_target = peer_id.to_string();
        pc.on_peer_connection_state_change(Box::new(move |state| {
            let target = state_target.clone();
            Box::pin(async move { eprintln!("[WebRTC] peer {target}: {state:?}"); })
        }));
        if let Some(audio) = &self.audio {
            let playback = audio.playback.clone();
            let remote_id = peer_id.to_string();
            pc.on_track(Box::new(move |track, _receiver, _transceiver| {
                let playback = playback.clone();
                let remote_id = remote_id.clone();
                Box::pin(async move {
                    // Each track gets its own Opus decoder; only remote audio reaches the mixer.
                    tokio::spawn(audio::decode_remote(remote_id, track, playback));
                })
            }));
        }
        if let Some(track) = &self.audio_track {
            let sender = pc.add_track(track.clone() as Arc<dyn TrackLocal + Send + Sync>).await.map_err(|e| format!("adicionar track Opus: {e}"))?;
            // Drain RTCP sender feedback so the WebRTC transport remains responsive.
            tokio::spawn(async move {
                while sender.read_rtcp().await.is_ok() {}
            });
        }
        // Data channel remains available for diagnostics and transport keepalive.
        pc.create_data_channel("livecall-control", None).await.map_err(|e| e.to_string())?;
        let mut peers = self.peers.lock().await;
        Ok(peers.entry(peer_id.to_string()).or_insert_with(|| pc.clone()).clone())
    }

    pub async fn create_offer(&self, peer_id: &str) -> Result<(), String> {
        let pc = self.peer(peer_id).await?;
        let offer = pc.create_offer(None).await.map_err(|e| e.to_string())?;
        pc.set_local_description(offer).await.map_err(|e| e.to_string())?;
        let local = pc.local_description().await.ok_or("SDP local ausente")?;
        send_signal(&self.outbound, peer_id, json!({"kind":"offer", "sdp":local.sdp}))
    }

    pub async fn handle_signal(&self, peer_id: &str, payload: Value) -> Result<(), String> {
        let kind = payload.get("kind").and_then(Value::as_str).ok_or("sinal WebRTC sem campo kind")?;
        match kind {
            "offer" => {
                let pc = self.peer(peer_id).await?;
                let sdp = payload.get("sdp").and_then(Value::as_str).ok_or("offer sem SDP")?;
                pc.set_remote_description(RTCSessionDescription::offer(sdp.to_string()).map_err(|e| e.to_string())?).await.map_err(|e| e.to_string())?;
                self.flush_pending_ice(peer_id, &pc).await?;
                let answer = pc.create_answer(None).await.map_err(|e| e.to_string())?;
                pc.set_local_description(answer).await.map_err(|e| e.to_string())?;
                let local = pc.local_description().await.ok_or("SDP answer local ausente")?;
                send_signal(&self.outbound, peer_id, json!({"kind":"answer", "sdp":local.sdp}))
            }
            "answer" => {
                let pc = self.peer(peer_id).await?;
                let sdp = payload.get("sdp").and_then(Value::as_str).ok_or("answer sem SDP")?;
                pc.set_remote_description(RTCSessionDescription::answer(sdp.to_string()).map_err(|e| e.to_string())?).await.map_err(|e| e.to_string())?;
                self.flush_pending_ice(peer_id, &pc).await
            }
            "ice_candidate" => {
                let pc = self.peer(peer_id).await?;
                let init = RTCIceCandidateInit {
                    candidate: payload.get("candidate").and_then(Value::as_str).unwrap_or_default().to_string(),
                    sdp_mid: payload.get("sdp_mid").and_then(Value::as_str).map(str::to_string),
                    sdp_mline_index: payload.get("sdp_mline_index").and_then(Value::as_u64).map(|v| v as u16),
                    username_fragment: None,
                };
                if pc.remote_description().await.is_some() {
                    pc.add_ice_candidate(init).await.map_err(|e| e.to_string())
                } else {
                    self.pending_ice.lock().await.entry(peer_id.to_string()).or_default().push(init);
                    Ok(())
                }
            }
            other => Err(format!("tipo de sinal WebRTC desconhecido: {other}")),
        }
    }

    async fn flush_pending_ice(&self, peer_id: &str, pc: &Arc<RTCPeerConnection>) -> Result<(), String> {
        let queued = self.pending_ice.lock().await.remove(peer_id).unwrap_or_default();
        for candidate in queued { pc.add_ice_candidate(candidate).await.map_err(|e| e.to_string())?; }
        Ok(())
    }

    pub async fn close_peer(&self, peer_id: &str) {
        self.pending_ice.lock().await.remove(peer_id);
        if let Some(audio) = &self.audio { audio.playback.remove(peer_id); }
        if let Some(pc) = self.peers.lock().await.remove(peer_id) { let _ = pc.close().await; }
    }
    pub async fn close_all(&self) {
        self.pending_ice.lock().await.clear();
        if let Some(audio) = &self.audio { audio.playback.clear(); audio.playback.set_deafened(false); audio.set_muted(true); }
        let peers = std::mem::take(&mut *self.peers.lock().await);
        for (_, pc) in peers { let _ = pc.close().await; }
    }
}

fn send_signal(tx: &UnboundedSender<String>, target: &str, payload: Value) -> Result<(), String> {
    let msg = json!({"type":"webrtc_signal", "target_user":target, "payload":payload});
    tx.send(msg.to_string()).map_err(|e| e.to_string())
}
