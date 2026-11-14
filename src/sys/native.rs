//! Escritorio (macOS, Windows, Linux): hilos, disco y `curl`.

use super::{MaybeSend, Request};
use image::RgbaImage;
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Ejecuta una tarea en otro hilo.
pub fn spawn(fut: impl Future<Output = ()> + MaybeSend + 'static) {
    std::thread::spawn(move || pollster::block_on(fut));
}

/// Guarda una fuente descargada en la carpeta de datos y devuelve su ruta.
pub fn save_font(name: &str, bytes: Vec<u8>) -> Result<PathBuf, String> {
    let dir = data_dir().join("fuentes");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let clean: String = name.chars().map(|c| if c == '/' || c == '\\' || c == ':' { '_' } else { c }).collect();
    let path = dir.join(clean);
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub async fn load_image(path: &Path) -> Result<RgbaImage, String> {
    crate::render::load_image(path)
}

// ---------------------------------------------------------------------------
// Red (con `curl`, incluido en macOS)

const AGENT: &str = "Carrusel/0.1 (+https://github.com/JoseLMendozaV/carrusel)";

pub async fn http(req: Request<'_>) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("curl");
    cmd.args(["-sS", "-L", "--fail-with-body", "--max-time", "25", "-A", AGENT]);
    if req.method != "GET" {
        cmd.args(["-X", req.method]);
    }
    for (name, value) in &req.headers {
        cmd.arg("-H").arg(format!("{name}: {value}"));
    }
    if req.body.is_some() {
        cmd.args(["--data-binary", "@-"]);
    }
    cmd.arg(req.url);
    cmd.stdin(if req.body.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|_| "No se encontró curl para conectarse a internet.".to_string())?;
    if let (Some(data), Some(mut input)) = (&req.body, child.stdin.take()) {
        input.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        let body = String::from_utf8_lossy(&out.stdout);
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("Sin conexión o error del servidor. {}", if body.trim().is_empty() { err.trim() } else { body.trim() })
            .chars()
            .take(240)
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Carpetas, caché y ajustes

/// Carpeta de caché (en macOS, ~/Library/Caches/Carrusel).
pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| if cfg!(target_os = "macos") { h.join("Library/Caches") } else { h.join(".cache") })
        .unwrap_or_else(std::env::temp_dir);
    base.join("Carrusel")
}

/// Carpeta de datos de la app (en macOS, ~/Library/Application Support/Carrusel).
pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|h| if cfg!(target_os = "macos") { h.join("Library/Application Support") } else { h.join(".local/share") })
        .unwrap_or_else(std::env::temp_dir);
    base.join("Carrusel")
}

/// Lo guardado en la caché con esta clave, si no tiene más de `max_age_days`.
pub fn cache_get(key: &str, max_age_days: Option<u64>) -> Option<Vec<u8>> {
    let path = cache_dir().join(key);
    let meta = std::fs::metadata(&path).ok()?;
    let fresh = max_age_days.is_none_or(|days| {
        meta.modified().ok().and_then(|m| m.elapsed().ok()).is_some_and(|age| age.as_secs() < days * 86_400)
    });
    if fresh { std::fs::read(&path).ok() } else { None }
}

pub fn cache_put(key: &str, data: &[u8]) {
    let path = cache_dir().join(key);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&path, data);
}

/// Ajuste privado (p. ej. la sesión de FreeSVG): un archivo legible solo por ti.
pub fn setting_get(key: &str) -> Option<String> {
    std::fs::read_to_string(data_dir().join(key)).ok().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

pub fn setting_set(key: &str, value: &str) -> Result<(), String> {
    let path = data_dir().join(key);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(&path, value).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn setting_remove(key: &str) {
    let _ = std::fs::remove_file(data_dir().join(key));
}
