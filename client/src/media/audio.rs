//! Captura CPAL -> Opus 48 kHz mono -> RTP WebRTC; RTP -> Opus -> mixer CPAL.
//! PCM never passes through the signaling server.
use std::{collections::{HashMap, VecDeque}, sync::{atomic::{AtomicBool, Ordering}, mpsc as std_mpsc, Arc, Mutex}, thread};
use cpal::{traits::{DeviceTrait, HostTrait, StreamTrait}, SampleFormat, StreamConfig};
use opus::{Application, Channels, Decoder, Encoder};
use tokio::sync::mpsc::Receiver;

const RATE: u32 = 48_000;
const FRAME: usize = 960; // 20 ms (Opus)
const MAX_QUEUED_SECONDS: usize = 1; // bound remote playback latency

/// Each remote participant has its own bounded buffer and local volume.
struct PeerAudio {
    samples: VecDeque<f32>,
    gain: f32,
}
impl Default for PeerAudio {
    fn default() -> Self { Self { samples: VecDeque::new(), gain: 1.0 } }
}

/// Each remote participant has its own bounded buffer; frames are mixed at output.
#[derive(Default)]
pub struct Playback {
    queues: Mutex<HashMap<String, PeerAudio>>,
    output_rate: u32,
    deafened: AtomicBool,
}
impl Playback {
    fn new(output_rate: u32) -> Self { Self { queues: Mutex::new(HashMap::new()), output_rate, deafened: AtomicBool::new(false) } }
    pub fn push(&self, peer: &str, samples: &[f32]) {
        if let Ok(mut all) = self.queues.lock() {
            let entry = all.entry(peer.to_string()).or_default();
            let q = &mut entry.samples;
            // Keep the most recent audio if the output device stalled.
            if q.len() + (samples.len() as u64 * self.output_rate as u64 / RATE as u64) as usize > self.output_rate as usize * MAX_QUEUED_SECONDS { q.clear(); }
            let ratio = RATE as f64 / self.output_rate as f64;
            let count = ((samples.len() as f64) / ratio).round() as usize;
            for n in 0..count {
                let pos = n as f64 * ratio;
                let a = pos.floor() as usize;
                if a >= samples.len() { break; }
                let b = (a + 1).min(samples.len() - 1);
                let frac = (pos - a as f64) as f32;
                q.push_back(samples[a] * (1.0 - frac) + samples[b] * frac);
            }
        }
    }
    /// Changes only local playback. The remote participant is not notified.
    pub fn set_peer_gain(&self, peer: &str, gain: f32) {
        if let Ok(mut all) = self.queues.lock() {
            all.entry(peer.to_string()).or_default().gain = gain.clamp(0.0, 1.0);
        }
    }
    pub fn remove(&self, peer: &str) { if let Ok(mut all) = self.queues.lock() { all.remove(peer); } }
    pub fn clear(&self) { if let Ok(mut all) = self.queues.lock() { all.clear(); } }
    pub fn set_deafened(&self, enabled: bool) {
        self.deafened.store(enabled, Ordering::Relaxed);
        // Discard buffered voice so unmuting does not replay old speech.
        if enabled { self.clear(); }
    }
    fn next(&self) -> f32 {
        if let Ok(mut all) = self.queues.try_lock() {
            let sample = all.values_mut().map(|entry| entry.samples.pop_front().unwrap_or(0.0) * entry.gain).sum::<f32>().clamp(-1.0, 1.0);
            if self.deafened.load(Ordering::Relaxed) { 0.0 } else { sample }
        } else { 0.0 }
    }
}

/// Enumerate actual CPAL capture endpoints. A webcam's microphone is a separate
/// audio device, so camera selection never changes this choice.
pub fn input_devices() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(|e| format!("listar microfones: {e}"))?;
    Ok(devices.enumerate().map(|(i, device)| {
        format!("{} · {}", i + 1, device.name().unwrap_or_else(|_| "Microfone sem nome".into()))
    }).collect())
}

pub fn active_input_label() -> Option<String> {
    let host = cpal::default_host();
    let desired = saved_input_name();
    let active = desired.and_then(|name| host.input_devices().ok()?.find(|d| d.name().ok().as_deref() == Some(name.as_str())))
        .or_else(|| host.default_input_device())?;
    let active_name = active.name().ok()?;
    input_devices().ok()?.into_iter().find(|label| label.split_once(" · ").is_some_and(|(_, n)| n == active_name))
}

fn input_by_label(label: &str) -> Result<cpal::Device, String> {
    let index = label.split_once(" · ").and_then(|(i, _)| i.parse::<usize>().ok())
        .and_then(|i| i.checked_sub(1)).ok_or("Seleção de microfone inválida")?;
    let devices = cpal::default_host().input_devices().map_err(|e| format!("listar microfones: {e}"))?;
    let device = devices.into_iter().nth(index).ok_or("Microfone desconectado: atualize a lista")?;
    let name = device.name().map_err(|e| format!("nome do microfone: {e}"))?;
    if label != format!("{} · {}", index + 1, name) { return Err("Lista de microfones mudou: atualize os dispositivos".into()); }
    Ok(device)
}

fn preference_path() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".config/livecall/microphone.txt"))
}
fn saved_input_name() -> Option<String> {
    std::fs::read_to_string(preference_path()?).ok().map(|s| s.trim().to_owned())
}
pub fn save_input_preference(label: &str) -> Result<(), String> {
    let path = preference_path().ok_or("HOME indisponível para salvar preferência")?;
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| e.to_string())?; }
    let name = label.split_once(" · ").map(|(_, n)| n).unwrap_or(label);
    std::fs::write(path, name).map_err(|e| format!("salvar microfone: {e}"))
}

enum DeviceCommand {
    Select(String, std_mpsc::SyncSender<Result<(), String>>),
    Shutdown,
}

pub struct AudioRuntime {
    pub playback: Arc<Playback>,
    muted: Arc<AtomicBool>,
    device_commands: std_mpsc::Sender<DeviceCommand>,
}
impl AudioRuntime {
    pub fn set_muted(&self, value: bool) { self.muted.store(value, Ordering::Relaxed); }
    /// Switches only the capture stream; output mixer, WebRTC track and mute stay intact.
    pub fn select_input(&self, label: String) -> Result<(), String> {
        let (tx, rx) = std_mpsc::sync_channel(1);
        self.device_commands.send(DeviceCommand::Select(label, tx)).map_err(|_| "thread de áudio encerrada".to_string())?;
        rx.recv_timeout(std::time::Duration::from_secs(5)).map_err(|e| format!("trocar microfone: {e}"))?
    }
    pub fn start() -> Result<(Self, Receiver<Vec<u8>>), String> {
        let host = cpal::default_host();
        let input = match saved_input_name() {
            Some(saved_name) => host.input_devices().ok().and_then(|mut devices| devices.find(|d| d.name().ok().as_deref() == Some(saved_name.as_str())))
                .or_else(|| host.default_input_device()),
            None => host.default_input_device(),
        }.ok_or("microfone padrão não encontrado")?;
        let output = host.default_output_device().ok_or("dispositivo de saída padrão não encontrado")?;
        let in_format = input.default_input_config().map_err(|e| format!("configuração do microfone: {e}"))?;
        let out_format = output.default_output_config().map_err(|e| format!("configuração do alto-falante: {e}"))?;
        let playback = Arc::new(Playback::new(out_format.sample_rate().0));
        let (pcm_tx, pcm_rx) = std_mpsc::sync_channel::<Vec<f32>>(12);
        // Bound encoded frames to ~240 ms; drop new frames instead of building latency.
        let (encoded_tx, encoded_rx) = tokio::sync::mpsc::channel(12);
        let (device_tx, device_rx) = std_mpsc::channel();
        let (ready_tx, ready_rx) = std_mpsc::sync_channel(1);
        let muted = Arc::new(AtomicBool::new(false));
        let capture_muted = muted.clone();
        let speaker = playback.clone();
        let switch_pcm_tx = pcm_tx.clone();
        thread::Builder::new().name("livecall-audio-device".into()).spawn(move || {
            // Keeping streams on the device thread avoids assumptions about Stream: Send.
            let built: Result<(cpal::Stream, cpal::Stream), String> = (|| {
                let input_stream = make_input(&input, in_format.config(), in_format.sample_format(), pcm_tx)?;
                let output_stream = make_output(&output, out_format.config(), out_format.sample_format(), speaker)?;
                input_stream.play().map_err(|e| format!("iniciar microfone: {e}"))?;
                output_stream.play().map_err(|e| format!("iniciar reprodução: {e}"))?;
                Ok((input_stream, output_stream))
            })();
            match built {
                Ok((mut capture, _speaker)) => {
                    let _ = ready_tx.send(Ok(()));
                    while let Ok(command) = device_rx.recv() {
                        match command {
                            DeviceCommand::Shutdown => break,
                            DeviceCommand::Select(label, reply) => {
                                let changed: Result<cpal::Stream, String> = (|| {
                                    let next = input_by_label(&label)?;
                                    let cfg = next.default_input_config().map_err(|e| format!("configurar microfone: {e}"))?;
                                    let stream = make_input(&next, cfg.config(), cfg.sample_format(), switch_pcm_tx.clone())?;
                                    stream.play().map_err(|e| format!("iniciar microfone: {e}"))?;
                                    Ok(stream)
                                })();
                                match changed {
                                    Ok(stream) => { capture = stream; let _ = reply.send(Ok(())); }
                                    Err(error) => { let _ = reply.send(Err(error)); }
                                }
                            }
                        }
                    }
                }
                Err(error) => { let _ = ready_tx.send(Err(error)); }
            }
        }).map_err(|e| format!("thread de áudio: {e}"))?;
        ready_rx.recv().map_err(|e| e.to_string())??;
        thread::Builder::new().name("livecall-opus-encoder".into()).spawn(move || {
            let mut encoder = match Encoder::new(RATE, Channels::Mono, Application::Voip) {
                Ok(v) => v, Err(e) => { eprintln!("[Áudio] encoder Opus: {e}"); return; }
            };
            let mut buffer: VecDeque<f32> = VecDeque::new();
            let mut encoded = [0u8; 4000];
            while let Ok(samples) = pcm_rx.recv() {
                buffer.extend(samples);
                while buffer.len() >= FRAME {
                    let mut frame = [0.0f32; FRAME];
                    for v in &mut frame { *v = buffer.pop_front().unwrap_or(0.0); }
                    if capture_muted.load(Ordering::Relaxed) { frame.fill(0.0); }
                    match encoder.encode_float(&frame, &mut encoded) {
                        Ok(n) => { match encoded_tx.try_send(encoded[..n].to_vec()) {
                            Ok(()) | Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {},
                            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => return,
                        } }
                        Err(e) => eprintln!("[Áudio] codificação Opus: {e}"),
                    }
                }
            }
        }).map_err(|e| format!("thread Opus: {e}"))?;
        Ok((Self { playback, muted, device_commands: device_tx }, encoded_rx))
    }
}
impl Drop for AudioRuntime {
    fn drop(&mut self) { let _ = self.device_commands.send(DeviceCommand::Shutdown); }
}

pub async fn decode_remote(peer: String, track: Arc<webrtc::track::track_remote::TrackRemote>, playback: Arc<Playback>) {
    let mut decoder = match Decoder::new(RATE, Channels::Mono) {
        Ok(v) => v, Err(e) => { eprintln!("[Áudio] decoder para {peer}: {e}"); return; }
    };
    let mut pcm = [0.0f32; 5760]; // maximum 120 ms Opus packet
    loop {
        match track.read_rtp().await {
            Ok((packet, _)) => {
                if packet.payload.is_empty() { continue; }
                match decoder.decode_float(&packet.payload, &mut pcm, false) {
                    Ok(count) => playback.push(&peer, &pcm[..count]),
                    Err(e) => eprintln!("[Áudio] decodificação de {peer}: {e}"),
                }
            }
            Err(_) => break,
        }
    }
    playback.remove(&peer);
}

fn make_input(device: &cpal::Device, config: StreamConfig, format: SampleFormat, tx: std_mpsc::SyncSender<Vec<f32>>) -> Result<cpal::Stream, String> {
    let channels = config.channels as usize;
    let sample_rate = config.sample_rate.0;
    // Fractional position is retained between callbacks for any device rate.
    let mut source_frames = 0u64;
    let mut next_sample = 0u64;
    let f32_tx = tx.clone();
    let mut emit = move |frame: &[f32]| {
        let mut mono = Vec::with_capacity((frame.len() / channels).max(1));
        for ch in frame.chunks_exact(channels) {
            let avg = ch.iter().sum::<f32>() / channels as f32;
            source_frames += 1;
            while next_sample * (sample_rate as u64) < source_frames * RATE as u64 {
                mono.push(avg);
                next_sample += 1;
            }
        }
        if !mono.is_empty() { let _ = f32_tx.try_send(mono); }
    };
    let err = |e| eprintln!("[Áudio] erro de captura: {e}");
    match format {
        SampleFormat::F32 => device.build_input_stream(&config, move |data: &[f32], _| emit(data), err, None),
        SampleFormat::I16 => {
            let mut source_frames = 0u64;
            let mut next_sample = 0u64;
            let tx = tx.clone();
            device.build_input_stream(&config, move |data: &[i16], _| {
                let mut mono = Vec::new();
                for frame in data.chunks_exact(channels) {
                    let avg = frame.iter().map(|v| *v as f32 / 32768.0).sum::<f32>() / channels as f32;
                    source_frames += 1;
                    while next_sample * (sample_rate as u64) < source_frames * RATE as u64 { mono.push(avg); next_sample += 1; }
                }
                if !mono.is_empty() { let _ = tx.try_send(mono); }
            }, err, None)
        }
        SampleFormat::U16 => {
            let mut source_frames = 0u64;
            let mut next_sample = 0u64;
            let tx = tx.clone();
            device.build_input_stream(&config, move |data: &[u16], _| {
                let mut mono = Vec::new();
                for frame in data.chunks_exact(channels) {
                    let avg = frame.iter().map(|v| (*v as f32 - 32768.0) / 32768.0).sum::<f32>() / channels as f32;
                    source_frames += 1;
                    while next_sample * (sample_rate as u64) < source_frames * RATE as u64 { mono.push(avg); next_sample += 1; }
                }
                if !mono.is_empty() { let _ = tx.try_send(mono); }
            }, err, None)
        }
        _ => return Err(format!("formato de captura não suportado: {format:?}")),
    }.map_err(|e| format!("abrir microfone: {e}"))
}

fn make_output(device: &cpal::Device, config: StreamConfig, format: SampleFormat, playback: Arc<Playback>) -> Result<cpal::Stream, String> {
    let channels = config.channels as usize;
    let err = |e| eprintln!("[Áudio] erro de reprodução: {e}");
    let playback_f32 = playback.clone();
    let playback_i16 = playback.clone();
    match format {
        SampleFormat::F32 => device.build_output_stream(&config, move |data: &mut [f32], _| {
            for frame in data.chunks_exact_mut(channels) { let v = playback_f32.next(); frame.fill(v); }
        }, err, None),
        SampleFormat::I16 => device.build_output_stream(&config, move |data: &mut [i16], _| {
            for frame in data.chunks_exact_mut(channels) { let v = (playback_i16.next() * i16::MAX as f32) as i16; frame.fill(v); }
        }, err, None),
        SampleFormat::U16 => device.build_output_stream(&config, move |data: &mut [u16], _| {
            for frame in data.chunks_exact_mut(channels) { let v = ((playback.next() * 0.5 + 0.5) * u16::MAX as f32) as u16; frame.fill(v); }
        }, err, None),
        _ => return Err(format!("formato de reprodução não suportado: {format:?}")),
    }.map_err(|e| format!("abrir alto-falante: {e}"))
}

#[cfg(test)]
mod playback_tests {
    use super::Playback;
    #[test]
    fn remote_volume_is_individual_and_cleared_on_leave() {
        let output = Playback::new(48_000);
        output.push("alice", &[0.5]);
        output.push("bob", &[0.5]);
        output.set_peer_gain("alice", 0.0);
        assert!((output.next() - 0.5).abs() < 0.001);
        output.remove("alice");
        output.push("alice", &[0.5]);
        assert!((output.next() - 0.5).abs() < 0.001);
    }
}
