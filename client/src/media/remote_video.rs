//! Remote VP8 RTP reception and FFmpeg decoding into RGBA frames for Iced.
//! RTP is reassembled into complete VP8 frames before writing an IVF stream.
use std::{collections::HashMap, sync::{Arc, Mutex}, time::Duration};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, process::Command};
use webrtc::track::track_remote::TrackRemote;

pub const WIDTH: u32 = 640;
pub const HEIGHT: u32 = 480;
const PIXELS: usize = (WIDTH * HEIGHT * 4) as usize;
const MAX_ENCODED_FRAME: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct VideoImage { pub rgba: Arc<Vec<u8>>, pub frame_number: u64 }

#[derive(Clone, Default)]
pub struct VideoFrames(Arc<Mutex<HashMap<String, VideoImage>>>);
impl VideoFrames {
    pub fn latest(&self, peer: &str) -> Option<VideoImage> { self.0.lock().ok()?.get(peer).cloned() }
    pub fn clear(&self, peer: &str) { if let Ok(mut frames) = self.0.lock() { frames.remove(peer); } }
    pub fn clear_all(&self) { if let Ok(mut frames) = self.0.lock() { frames.clear(); } }
    fn store(&self, peer: &str, image: VideoImage) { if let Ok(mut frames) = self.0.lock() { frames.insert(peer.to_owned(), image); } }
}

// RFC 7741: skip the VP8 payload descriptor (including optional X extensions).
fn strip_vp8_descriptor(payload: &[u8]) -> Option<(&[u8], bool)> {
    let first = *payload.first()?;
    let start_of_partition = first & 0x10 != 0 && first & 0x0f == 0;
    let mut pos = 1usize;
    if first & 0x80 != 0 {
        let x = *payload.get(pos)?;
        pos += 1;
        if x & 0x80 != 0 { // PictureID: one or two bytes
            let picture = *payload.get(pos)?;
            pos += if picture & 0x80 != 0 { 2 } else { 1 };
        }
        if x & 0x40 != 0 { pos += 1; } // TL0PICIDX
        if x & 0x20 != 0 || x & 0x10 != 0 { pos += 1; } // T/K shared octet
    }
    let body = payload.get(pos..)?;
    if body.is_empty() { return None; }
    Some((body, start_of_partition))
}

fn ivf_header() -> [u8; 32] {
    let mut h = [0u8; 32];
    h[..4].copy_from_slice(b"DKIF");
    h[6..8].copy_from_slice(&32u16.to_le_bytes());
    h[8..12].copy_from_slice(b"VP80");
    h[12..14].copy_from_slice(&(WIDTH as u16).to_le_bytes());
    h[14..16].copy_from_slice(&(HEIGHT as u16).to_le_bytes());
    h[16..20].copy_from_slice(&15u32.to_le_bytes());
    h[20..24].copy_from_slice(&1u32.to_le_bytes());
    h
}

/// One process per inbound video track; FFmpeg stays running across inter-frames.
/// On packet loss, the incomplete access unit is discarded, rather than producing corrupt IVF.
pub async fn decode_remote(peer: String, track: Arc<TrackRemote>, frames: VideoFrames) {
    let mut child = match Command::new("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-f", "ivf", "-i", "pipe:0",
            "-an", "-f", "rawvideo", "-pix_fmt", "rgba", "-vsync", "0", "pipe:1"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn() {
            Ok(child) => child,
            Err(error) => { eprintln!("[Vídeo] decoder FFmpeg indisponível para {peer}: {error}"); return; }
        };
    let Some(mut stdin) = child.stdin.take() else { let _ = child.kill().await; return; };
    let Some(mut stdout) = child.stdout.take() else { let _ = child.kill().await; return; };
    let decoder_peer = peer.clone();
    let decoder_frames = frames.clone();
    let reader = tokio::spawn(async move {
        let mut index = 0u64;
        loop {
            let mut rgba = vec![0u8; PIXELS];
            if stdout.read_exact(&mut rgba).await.is_err() { break; }
            index += 1;
            decoder_frames.store(&decoder_peer, VideoImage { rgba: Arc::new(rgba), frame_number: index });
        }
    });
    if stdin.write_all(&ivf_header()).await.is_err() { reader.abort(); let _ = child.kill().await; return; }

    let mut current = Vec::new();
    let mut expected_sequence: Option<u16> = None;
    let mut timestamp: Option<u32> = None;
    let mut valid = false;
    let mut sequence = 0u64;
    loop {
        let Ok((packet, _)) = track.read_rtp().await else { break; };
        let seq = packet.header.sequence_number;
        let ts = packet.header.timestamp;
        if timestamp != Some(ts) {
            current.clear();
            valid = false;
            timestamp = Some(ts);
        }
        if expected_sequence.is_some_and(|expected| expected != seq) {
            current.clear();
            valid = false;
        }
        expected_sequence = Some(seq.wrapping_add(1));
        let Some((body, start)) = strip_vp8_descriptor(packet.payload.as_ref()) else { continue; };
        if start {
            current.clear();
            valid = true;
        }
        if valid && current.len() + body.len() <= MAX_ENCODED_FRAME {
            current.extend_from_slice(body);
        } else { valid = false; current.clear(); }
        if packet.header.marker {
            if valid && !current.is_empty() {
                let mut h = [0u8; 12];
                h[..4].copy_from_slice(&(current.len() as u32).to_le_bytes());
                h[4..].copy_from_slice(&sequence.to_le_bytes());
                sequence += 1;
                if stdin.write_all(&h).await.is_err() || stdin.write_all(&current).await.is_err() { break; }
            }
            valid = false;
            current.clear();
        }
    }
    drop(stdin);
    reader.abort();
    if tokio::time::timeout(Duration::from_secs(1), child.wait()).await.is_err() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    frames.clear(&peer);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn descriptor_without_extensions() {
        assert_eq!(strip_vp8_descriptor(&[0x10, 0x11, 0x22]), Some((&[0x11, 0x22][..], true)));
    }
    #[test]
    fn descriptor_with_picture_id_and_tid() {
        assert_eq!(strip_vp8_descriptor(&[0x90, 0xa0, 0x81, 0x03, 0x00, 0xcc]), Some((&[0xcc][..], true)));
    }
    #[test]
    fn ivf_headers_are_consistent() {
        let header = ivf_header();
        assert_eq!(&header[0..4], b"DKIF");
        assert_eq!(&header[8..12], b"VP80");
        assert_eq!(u16::from_le_bytes([header[12],header[13]]), 640);
    }
}
