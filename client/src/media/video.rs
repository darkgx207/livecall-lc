//! Camera capture and VP8 encoding through an optional FFmpeg executable.
//! This module reads IVF frames; it does not decode or render video.
use std::{process::Stdio, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};
use tokio::{io::{AsyncRead, AsyncReadExt}, process::Command, sync::{broadcast, watch, Mutex}};

const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraDevice {
    pub path: String,
    pub name: String,
    pub stable_id: String,
}
impl CameraDevice {
    pub fn label(&self) -> String { format!("{} ({})", self.name, self.path) }
}

/// Linux: only V4L2 video-capture nodes are shown (not metadata-only nodes).
/// Virtual V4L2 cameras such as OBS are included if they expose capture capability.
#[cfg(target_os = "linux")]
pub fn list_cameras() -> Result<Vec<CameraDevice>, String> {
    use std::{fs, fs::OpenOptions, os::fd::AsRawFd};
    #[repr(C)]
    #[derive(Default)]
    struct V4l2Capability {
        driver: [u8; 16], card: [u8; 32], bus_info: [u8; 32],
        version: u32, capabilities: u32, device_caps: u32, reserved: [u32; 3],
    }
    fn cstr(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes.split(|b| *b == 0).next().unwrap_or_default()).trim().to_string()
    }
    let entries = fs::read_dir("/dev").map_err(|e| format!("Falha ao listar /dev: {e}"))?;
    let mut cameras = Vec::new();
    for entry in entries.flatten() {
        let filename = entry.file_name().to_string_lossy().to_string();
        if !filename.starts_with("video") || !filename[5..].chars().all(|c| c.is_ascii_digit()) || filename.len() == 5 { continue; }
        let path = format!("/dev/{filename}");
        // O_RDWR is not required for VIDIOC_QUERYCAP, so readonly users can enumerate.
        let file = match OpenOptions::new().read(true).open(&path) {
            Ok(file) => file,
            Err(_) => continue,
        };
        let mut cap = V4l2Capability::default();
        // _IOR('V', 0, struct v4l2_capability), size=104 (Linux ioctl ABI).
        let result = unsafe { libc::ioctl(file.as_raw_fd(), 0x8068_5600 as libc::c_ulong, &mut cap) };
        if result < 0 { continue; }
        let capabilities = if cap.capabilities & 0x8000_0000 != 0 { cap.device_caps } else { cap.capabilities };
        // V4L2_CAP_VIDEO_CAPTURE | V4L2_CAP_VIDEO_CAPTURE_MPLANE.
        if capabilities & (0x0000_0001 | 0x0000_1000) == 0 { continue; }
        // Ordinary webcam capture is single-plane. Multi-plane-only devices may not work via FFmpeg.
        let name = cstr(&cap.card);
        let bus = cstr(&cap.bus_info);
        let stable_id = if bus.is_empty() { format!("name:{name}") } else { format!("bus:{bus};name:{name}") };
        cameras.push(CameraDevice { path, name: if name.is_empty() { filename.clone() } else { name }, stable_id });
    }
    cameras.sort_by(|a,b| a.path.cmp(&b.path));
    Ok(cameras)
}

#[cfg(not(target_os = "linux"))]
pub fn list_cameras() -> Result<Vec<CameraDevice>, String> {
    // Windows device discovery requires a DirectShow/Media Foundation backend.
    // Do not invent a default camera device.
    Err("Detecção automática de câmeras disponível inicialmente apenas no Linux; no Windows use LIVECALL_CAMERA_DEVICE até a implementação específica".into())
}

#[cfg(target_os = "linux")]
fn settings_file() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
        Some(PathBuf::from(xdg).join("livecall/camera-device.txt"))
    } else {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/livecall/camera-device.txt"))
    }
}
#[cfg(not(target_os = "linux"))]
fn settings_file() -> Option<std::path::PathBuf> { None }

pub fn preferred_camera_id() -> Option<String> {
    settings_file().and_then(|path| std::fs::read_to_string(path).ok()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn save_preferred_camera(camera: &CameraDevice) -> Result<(), String> {
    let Some(path) = settings_file() else { return Ok(()); };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Não foi possível criar pasta de configurações: {e}"))?;
    }
    std::fs::write(&path, &camera.stable_id).map_err(|e| format!("Não foi possível salvar a câmera preferida: {e}"))
}


#[derive(Clone, Debug)]
pub struct Vp8Frame { pub data: Vec<u8>, pub duration: Duration }

pub struct CameraRuntime {
    frames: broadcast::Sender<Vp8Frame>,
    stop: Mutex<Option<watch::Sender<bool>>>,
    running: Arc<AtomicBool>,
}
impl CameraRuntime {
    pub fn new() -> Self {
        let (frames, _) = broadcast::channel(4);
        Self { frames, stop: Mutex::new(None), running: Arc::new(AtomicBool::new(false)) }
    }
    pub fn subscribe(&self) -> broadcast::Receiver<Vp8Frame> { self.frames.subscribe() }
    pub fn is_running(&self) -> bool { self.running.load(Ordering::Acquire) }

    pub async fn start(&self, selected_device: &str) -> Result<(), String> {
        let mut stop_guard = self.stop.lock().await;
        if self.is_running() { return Ok(()); }
        // FFmpeg is an optional external executable: no system sound settings are changed.
        if selected_device.trim().is_empty() { return Err("Nenhuma câmera selecionada. Atualize a lista de dispositivos.".into()); }
        let device = selected_device.to_string();
        let mut command = Command::new("ffmpeg");
        command.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-f"]);
        if cfg!(target_os = "windows") { command.arg("dshow"); } else { command.arg("v4l2"); }
        command.args(["-framerate", "15", "-video_size", "640x480", "-i", &device,
            "-an", "-c:v", "libvpx", "-deadline", "realtime", "-cpu-used", "8",
            "-b:v", "450k", "-g", "30", "-pix_fmt", "yuv420p",
            "-f", "ivf", "pipe:1"]);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|e| format!("FFmpeg não iniciou ({e}); instale ffmpeg e configure LIVECALL_CAMERA_DEVICE"))?;
        let mut stdout = child.stdout.take().ok_or("Saída de vídeo FFmpeg indisponível")?;
        let mut stderr = child.stderr.take().ok_or("Diagnóstico do FFmpeg indisponível")?;
        // Validate the IVF header before announcing the camera as active.
        let mut header = [0u8; 32];
        let ready = tokio::time::timeout(Duration::from_secs(8), stdout.read_exact(&mut header)).await;
        if !matches!(ready, Ok(Ok(_))) {
            let _ = child.kill().await;
            let mut details = String::new();
            let _ = tokio::time::timeout(Duration::from_secs(2), stderr.read_to_string(&mut details)).await;
            let reason = summarize_ffmpeg_error(&details);
            let _ = child.wait().await;
            return Err(format!("A câmera não pôde ser iniciada ({device}): {reason}"));
        }
        if &header[0..4] != b"DKIF" || &header[8..12] != b"VP80" {
            let _ = child.kill().await;
            return Err("FFmpeg produziu vídeo diferente de IVF/VP8".into());
        }
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let frames = self.frames.clone();
        let running = self.running.clone();
        running.store(true, Ordering::Release);
        *stop_guard = Some(stop_tx);
        tokio::spawn(async move {
            let stderr_log = tokio::spawn(async move {
                let mut details = String::new();
                let _ = stderr.read_to_string(&mut details).await;
                if !details.trim().is_empty() { eprintln!("[Câmera/FFmpeg] {}", details.trim()); }
            });
            loop {
                tokio::select! {
                    _ = stop_rx.changed() => { if *stop_rx.borrow() { break; } },
                    frame = read_ivf_frame(&mut stdout) => {
                        match frame {
                            Ok(frame) => { let _ = frames.send(frame); },
                            Err(error) => { eprintln!("[Câmera] FFmpeg encerrado: {error}"); break; }
                        }
                    }
                }
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            stderr_log.abort();
            running.store(false, Ordering::Release);
        });
        Ok(())
    }
    pub async fn stop(&self) {
        if let Some(stop) = self.stop.lock().await.take() { let _ = stop.send(true); }
        self.running.store(false, Ordering::Release);
    }
}

fn summarize_ffmpeg_error(stderr: &str) -> String {
    let lower = stderr.to_lowercase();
    if lower.contains("permission denied") { return "permissão negada para acessar a câmera".into(); }
    if lower.contains("device or resource busy") { return "dispositivo ocupado por outro aplicativo".into(); }
    if lower.contains("no such file or directory") { return "dispositivo não existe ou foi desconectado".into(); }
    if lower.contains("unknown encoder") || lower.contains("encoder not found") { return "FFmpeg não possui o encoder VP8 libvpx".into(); }
    if lower.contains("invalid argument") || lower.contains("not supported") { return "formato 640x480 a 15 fps não suportado pelo dispositivo".into(); }
    stderr.lines().rev().find(|s| !s.trim().is_empty()).unwrap_or("verifique webcam, permissões, FFmpeg e formato de captura").trim().chars().take(350).collect()
}

async fn read_ivf_frame<R: AsyncRead + Unpin>(reader: &mut R) -> Result<Vp8Frame, String> {
    let mut header = [0u8; 12];
    reader.read_exact(&mut header).await.map_err(|e| e.to_string())?;
    let size = u32::from_le_bytes(header[0..4].try_into().unwrap()) as usize;
    if size == 0 || size > MAX_FRAME_BYTES { return Err(format!("quadro VP8 inválido: {size} bytes")); }
    let mut data = vec![0; size];
    reader.read_exact(&mut data).await.map_err(|e| e.to_string())?;
    Ok(Vp8Frame { data, duration: Duration::from_millis(67) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ffmpeg_errors_are_human_readable() {
        assert!(summarize_ffmpeg_error("/dev/video2: Device or resource busy").contains("ocupado"));
        assert!(summarize_ffmpeg_error("/dev/video0: Permission denied").contains("permissão"));
        assert!(summarize_ffmpeg_error("/dev/video0: No such file or directory").contains("não existe"));
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn camera_scan_handles_no_devices() {
        // On machines without cameras, an empty list is successful detection.
        let result = list_cameras();
        assert!(result.is_ok() || result.as_ref().err().is_some_and(|s| s.contains("/dev")));
    }
    #[tokio::test]
    async fn ivf_frame_reader() {
        let mut data = vec![];
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&0u64.to_le_bytes());
        data.extend_from_slice(&[1,2,3]);
        let frame = read_ivf_frame(&mut data.as_slice()).await.unwrap();
        assert_eq!(frame.data, vec![1,2,3]);
    }
    #[tokio::test]
    async fn rejects_empty_frame() {
        let data = [0u8;12];
        assert!(read_ivf_frame(&mut data.as_slice()).await.is_err());
    }
}
