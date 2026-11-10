//! Video con ffmpeg: información, fotogramas, reproducción en el editor y
//! codificación de MP4 compatibles con Instagram y TikTok
//! (H.264 High, yuv420p, AAC 48 kHz, 30 fps, «faststart»).

use image::RgbaImage;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

pub const FPS: u32 = 30;
/// Instagram admite videos de hasta 60 s por slide de carrusel.
pub const MAX_SLIDE_SECONDS: f32 = 60.0;

pub const VIDEO_EXT: &[&str] =
    &["mp4", "mov", "m4v", "avi", "mkv", "webm", "wmv", "flv", "3gp", "mpg", "mpeg", "ts", "mts", "m2ts", "ogv"];

pub fn is_video(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXT.contains(&e.to_lowercase().as_str()))
}

/// Busca un programa en las rutas de Homebrew y luego en el PATH. Las apps
/// abiertas desde Finder no heredan el PATH de la terminal.
fn find_tool(name: &str) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> =
        ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/opt/local/bin"].iter().map(PathBuf::from).collect();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    let exe = if cfg!(windows) { format!("{name}.exe") } else { name.to_string() };
    dirs.into_iter().map(|d| d.join(&exe)).find(|p| p.is_file())
}

pub fn ffmpeg() -> Option<&'static Path> {
    static P: OnceLock<Option<PathBuf>> = OnceLock::new();
    P.get_or_init(|| find_tool("ffmpeg")).as_deref()
}

fn ffprobe() -> Option<&'static Path> {
    static P: OnceLock<Option<PathBuf>> = OnceLock::new();
    P.get_or_init(|| find_tool("ffprobe")).as_deref()
}

pub const MISSING: &str = "Para usar videos instala ffmpeg (en la terminal: brew install ffmpeg) y vuelve a abrir Carrusel.";

fn ffmpeg_cmd() -> Result<Command, String> {
    let mut c = Command::new(ffmpeg().ok_or(MISSING)?);
    c.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
    Ok(c)
}

#[derive(Clone, Copy, Debug)]
pub struct VideoInfo {
    /// Tamaño ya girado como se ve (los videos del iPhone vienen rotados).
    pub width: u32,
    pub height: u32,
    pub duration: f32,
    pub has_audio: bool,
}

impl VideoInfo {
    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height.max(1) as f32
    }
}

pub fn probe(path: &Path) -> Result<VideoInfo, String> {
    let out = Command::new(ffprobe().ok_or(MISSING)?)
        .args(["-v", "error", "-show_streams", "-show_format", "-of", "json"])
        .arg(path)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("{}: no es un video válido", path.display()));
    }
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let streams = json["streams"].as_array().cloned().unwrap_or_default();
    let v = streams
        .iter()
        .find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1)
        .ok_or_else(|| format!("{}: no tiene imagen de video", path.display()))?;
    let mut width = v["width"].as_u64().unwrap_or(0) as u32;
    let mut height = v["height"].as_u64().unwrap_or(0) as u32;
    // Píxeles no cuadrados (algunos videos de cámaras antiguas).
    if let Some((a, b)) = v["sample_aspect_ratio"].as_str().and_then(|s| s.split_once(':')) {
        if let (Ok(a), Ok(b)) = (a.parse::<f32>(), b.parse::<f32>()) {
            if a > 0.0 && b > 0.0 && a != b {
                width = (width as f32 * a / b).round() as u32;
            }
        }
    }
    let rotation = v["side_data_list"]
        .as_array()
        .and_then(|l| l.iter().find_map(|d| d["rotation"].as_f64()))
        .or_else(|| v["tags"]["rotate"].as_str().and_then(|r| r.parse().ok()))
        .unwrap_or(0.0);
    if (rotation.abs() as i64) % 180 == 90 {
        std::mem::swap(&mut width, &mut height);
    }
    let duration = json["format"]["duration"]
        .as_str()
        .or(v["duration"].as_str())
        .and_then(|d| d.parse::<f32>().ok())
        .unwrap_or(0.0);
    if width == 0 || height == 0 {
        return Err(format!("{}: no se pudo leer el tamaño del video", path.display()));
    }
    let has_audio = streams.iter().any(|s| s["codec_type"] == "audio");
    Ok(VideoInfo { width, height, duration, has_audio })
}

/// Tamaño par (los codificadores lo exigen) que cabe en `max` manteniendo la proporción.
pub fn fit_size(info: &VideoInfo, max: u32) -> (u32, u32) {
    let s = (max as f32 / info.width.max(info.height) as f32).min(1.0);
    let even = |v: f32| ((v / 2.0).round() as u32 * 2).max(2);
    (even(info.width as f32 * s), even(info.height as f32 * s))
}

/// Un fotograma en el segundo `t`, escalado a `w × h`.
pub fn frame_at(path: &Path, t: f32, (w, h): (u32, u32)) -> Result<RgbaImage, String> {
    let out = ffmpeg_cmd()?
        .args(["-ss", &format!("{t:.3}"), "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-vf", &format!("scale={w}:{h}"), "-f", "rawvideo", "-pix_fmt", "rgba", "-"])
        .output()
        .map_err(|e| e.to_string())?;
    RgbaImage::from_raw(w, h, out.stdout).ok_or_else(|| format!("{}: no se pudo leer el fotograma", path.display()))
}

/// Lee fotogramas RGBA de un video, uno tras otro, a 30 fps y en bucle.
pub struct FrameReader {
    child: Child,
    out: ChildStdout,
    size: (u32, u32),
    last: Option<RgbaImage>,
}

impl FrameReader {
    /// `realtime` entrega los fotogramas a velocidad normal (para reproducir).
    pub fn spawn(path: &Path, start: f32, size: (u32, u32), realtime: bool) -> Result<Self, String> {
        let mut cmd = ffmpeg_cmd()?;
        if realtime {
            cmd.arg("-re");
        }
        cmd.args(["-stream_loop", "-1", "-ss", &format!("{start:.3}"), "-i"])
            .arg(path)
            .args([
                "-an",
                "-vf",
                &format!("fps={FPS},scale={}:{}", size.0, size.1),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let out = child.stdout.take().ok_or("sin salida de ffmpeg")?;
        Ok(Self { child, out, size, last: None })
    }

    /// El siguiente fotograma; si el video se corta, repite el último.
    pub fn next_frame(&mut self) -> Option<&RgbaImage> {
        let mut buf = vec![0u8; (self.size.0 * self.size.1 * 4) as usize];
        if self.out.read_exact(&mut buf).is_ok() {
            self.last = RgbaImage::from_raw(self.size.0, self.size.1, buf);
        }
        self.last.as_ref()
    }
}

impl Drop for FrameReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Reproduce un video en segundo plano para verlo moverse en el editor.
pub struct Player {
    pub frame: Arc<Mutex<Option<RgbaImage>>>,
    stop: Arc<AtomicBool>,
    pub path: PathBuf,
    pub start: f32,
}

impl Player {
    pub fn new(path: &Path, start: f32, size: (u32, u32), on_frame: impl Fn() + Send + 'static) -> Result<Self, String> {
        let mut reader = FrameReader::spawn(path, start, size, true)?;
        let frame = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (f, s) = (frame.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                let Some(img) = reader.next_frame().cloned() else { break };
                *f.lock().unwrap() = Some(img);
                on_frame();
            }
        });
        Ok(Self { frame, stop, path: path.to_path_buf(), start })
    }

    pub fn take_frame(&self) -> Option<RgbaImage> {
        self.frame.lock().ok()?.take()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Pista de audio que acompaña al video exportado.
pub struct Audio {
    pub path: PathBuf,
    pub start: f32,
    /// Repetir el audio si es más corto que el video.
    pub looped: bool,
}

/// Codifica fotogramas RGBA en un MP4 listo para Instagram y TikTok.
pub struct Encoder {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Encoder {
    pub fn new(out: &Path, (w, h): (u32, u32), duration: f32, audio: Option<&Audio>) -> Result<Self, String> {
        let mut cmd = ffmpeg_cmd()?;
        cmd.arg("-y")
            .args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{h}"), "-r", &FPS.to_string(), "-i", "-"]);
        if let Some(a) = audio {
            if a.looped {
                cmd.args(["-stream_loop", "-1"]);
            }
            cmd.args(["-ss", &format!("{:.3}", a.start), "-i"]).arg(&a.path);
            cmd.args(["-map", "0:v:0", "-map", "1:a:0?"]);
        }
        cmd.args([
            "-c:v", "libx264", "-preset", "medium", "-crf", "18", "-profile:v", "high", "-pix_fmt", "yuv420p",
            "-r", &FPS.to_string(), "-g", &(FPS * 2).to_string(),
            "-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-ac", "2",
            "-t", &format!("{duration:.3}"),
            "-movflags", "+faststart",
        ])
        .arg(out)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| e.to_string())?;
        let stdin = child.stdin.take();
        Ok(Self { child, stdin })
    }

    pub fn write(&mut self, frame: &RgbaImage) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("el codificador ya se cerró")?;
        stdin.write_all(frame.as_raw()).map_err(|_| "ffmpeg dejó de aceptar fotogramas".to_string())
    }

    pub fn finish(mut self) -> Result<(), String> {
        drop(self.stdin.take());
        let out = self.child.wait_with_output().map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!("ffmpeg: {}", String::from_utf8_lossy(&out.stderr).trim()))
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    /// Crea un video de prueba (barras de color + tono) si hay ffmpeg.
    pub fn sample_video(dir: &Path, name: &str, size: (u32, u32), seconds: f32) -> Option<PathBuf> {
        ffmpeg()?;
        let path = dir.join(name);
        let ok = ffmpeg_cmd()
            .ok()?
            .args(["-y", "-f", "lavfi", "-i", &format!("testsrc2=size={}x{}:rate=30:duration={seconds}", size.0, size.1)])
            .args(["-f", "lavfi", "-i", &format!("sine=frequency=440:duration={seconds}")])
            .args(["-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"])
            .arg(&path)
            .status()
            .ok()?
            .success();
        ok.then_some(path)
    }

    #[test]
    fn lee_informacion_y_fotogramas() {
        let dir = std::env::temp_dir().join("carrusel_video_test");
        std::fs::create_dir_all(&dir).unwrap();
        let Some(path) = sample_video(&dir, "barras.mov", (640, 360), 2.0) else {
            eprintln!("sin ffmpeg: prueba omitida");
            return;
        };
        let info = probe(&path).unwrap();
        assert_eq!((info.width, info.height), (640, 360));
        assert!((info.duration - 2.0).abs() < 0.2, "{}", info.duration);
        assert!(info.has_audio);
        let frame = frame_at(&path, 1.0, (320, 180)).unwrap();
        assert_eq!(frame.dimensions(), (320, 180));
        let mut r = FrameReader::spawn(&path, 0.0, (160, 90), false).unwrap();
        for _ in 0..70 {
            // Más fotogramas de los que tiene: el bucle sigue entregando.
            assert!(r.next_frame().is_some());
        }
        assert!(is_video(Path::new("a.MOV")) && !is_video(Path::new("a.jpg")));
    }
}
