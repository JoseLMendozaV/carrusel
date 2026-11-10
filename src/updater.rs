//! Actualizaciones: compara la versión instalada con el último release de
//! GitHub, descarga el .zip, verifica su huella SHA-256 y su firma, y cambia
//! la app por la nueva al reiniciar.

use crate::online;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const REPO: &str = "JoseLMendozaV/carrusel";
pub const RELEASES_PAGE: &str = "https://github.com/JoseLMendozaV/carrusel/releases";

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    pub asset_url: String,
    pub asset_size: u64,
    /// Huella SHA-256 que publica GitHub (o la de las notas del release).
    pub sha256: Option<String>,
}

/// «v1.2.3» o «1.2.3» → (1, 2, 3).
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let mut it = s.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).map(|p| p.parse::<u64>());
    Some((it.next()?.ok()?, it.next().unwrap_or(Ok(0)).ok()?, it.next().unwrap_or(Ok(0)).ok()?))
}

pub fn is_newer(remote: &str, local: &str) -> bool {
    matches!((parse_version(remote), parse_version(local)), (Some(r), Some(l)) if r > l)
}

/// Busca «SHA-256 … `hex`» en las notas del release.
fn sha_from_notes(notes: &str) -> Option<String> {
    notes.lines().filter(|l| l.contains("SHA-256")).find_map(|l| {
        l.split('`').map(str::trim).find(|t| t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit())).map(str::to_lowercase)
    })
}

fn parse_release(json: &serde_json::Value) -> Option<Release> {
    let tag = json["tag_name"].as_str()?;
    if json["draft"].as_bool() == Some(true) || json["prerelease"].as_bool() == Some(true) {
        return None;
    }
    let notes = json["body"].as_str().unwrap_or("").to_string();
    let asset = json["assets"].as_array()?.iter().find(|a| a["name"].as_str().is_some_and(|n| n.ends_with("-macOS.zip")))?;
    let sha256 = asset["digest"]
        .as_str()
        .and_then(|d| d.strip_prefix("sha256:"))
        .map(str::to_lowercase)
        .or_else(|| sha_from_notes(&notes));
    Some(Release {
        version: tag.trim_start_matches(['v', 'V']).to_string(),
        page: json["html_url"].as_str().unwrap_or(RELEASES_PAGE).to_string(),
        asset_url: asset["browser_download_url"].as_str()?.to_string(),
        asset_size: asset["size"].as_u64().unwrap_or(0),
        notes,
        sha256,
    })
}

/// El último release publicado (sin borradores ni versiones de prueba).
pub fn latest() -> Result<Release, String> {
    let data = online::get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))?;
    let json: serde_json::Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    parse_release(&json).ok_or_else(|| "El último release no trae la app para Mac.".to_string())
}

// ---------------------------------------------------------------------------
// Comprobación automática: como mucho una vez al día

fn stamp_path() -> PathBuf {
    online::data_dir().join("ultima_comprobacion_actualizaciones")
}

pub fn due_for_check() -> bool {
    std::fs::metadata(stamp_path())
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_none_or(|age| age.as_secs() > 24 * 3600)
}

pub fn mark_checked() {
    let path = stamp_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, current_version());
}

// ---------------------------------------------------------------------------
// Instalación

/// El paquete `Carrusel.app` desde el que se está ejecutando la app, si lo hay
/// (con `cargo run` no hay paquete y la actualización se hace a mano).
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

pub fn current_bundle() -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    bundle_of(&std::env::current_exe().ok()?.canonicalize().ok()?)
}

/// ¿Se puede sustituir el paquete sin pedir permisos de administrador?
pub fn can_replace(bundle: &Path) -> bool {
    let Some(parent) = bundle.parent() else { return false };
    let probe = parent.join(format!(".carrusel-prueba-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(probe);
    ok
}

fn sha256_of(path: &Path) -> Result<String, String> {
    let out = Command::new("shasum").args(["-a", "256"]).arg(path).output().map_err(|e| e.to_string())?;
    String::from_utf8_lossy(&out.stdout).split_whitespace().next().map(str::to_lowercase).ok_or("No se pudo calcular la huella.".into())
}

fn bundle_version(app: &Path) -> Option<String> {
    let plist = std::fs::read_to_string(app.join("Contents/Info.plist")).ok()?;
    let after = &plist[plist.find("CFBundleShortVersionString")?..];
    let start = after.find("<string>")? + 8;
    Some(after[start..start + after[start..].find("</string>")?].trim().to_string())
}

/// Descarga y prepara la versión nueva. Devuelve la ruta de la app nueva,
/// ya verificada. `progress` recibe la fracción descargada (0 a 1).
pub fn download(release: &Release, progress: &dyn Fn(f32)) -> Result<PathBuf, String> {
    let dir = online::cache_dir().join("actualizacion").join(&release.version);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let zip = dir.join("Carrusel.zip");

    // curl en segundo plano y el progreso según lo que va ocupando el archivo.
    let mut child = Command::new("curl")
        .args(["-sS", "-L", "--fail", "--max-time", "900", "-o"])
        .arg(&zip)
        .arg(&release.asset_url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "No se encontró curl para descargar la actualización.".to_string())?;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            if !status.success() {
                return Err("No se pudo descargar la actualización: revisa tu conexión.".into());
            }
            break;
        }
        if release.asset_size > 0 {
            let got = std::fs::metadata(&zip).map(|m| m.len()).unwrap_or(0);
            progress((got as f32 / release.asset_size as f32).min(0.99));
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }

    // Integridad: la huella tiene que coincidir con la publicada.
    match &release.sha256 {
        Some(expected) => {
            let got = sha256_of(&zip)?;
            if &got != expected {
                let _ = std::fs::remove_dir_all(&dir);
                return Err("La descarga no coincide con la huella publicada; no se instaló nada.".into());
            }
        }
        None => return Err("El release no publica su huella SHA-256; actualiza a mano desde GitHub.".into()),
    }

    let out = dir.join("nueva");
    let status = Command::new("ditto").args(["-x", "-k"]).arg(&zip).arg(&out).status().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("No se pudo descomprimir la actualización.".into());
    }
    let app = std::fs::read_dir(&out)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().is_some_and(|e| e == "app"))
        .ok_or("El .zip no contiene la app.")?;
    let signed = Command::new("codesign").args(["--verify", "--deep", "--strict"]).arg(&app).status().map(|s| s.success()).unwrap_or(false);
    if !signed {
        return Err("La app descargada no tiene una firma válida; no se instaló.".into());
    }
    if bundle_version(&app).as_deref() != Some(release.version.as_str()) {
        return Err("La app descargada no es la versión anunciada; no se instaló.".into());
    }
    progress(1.0);
    Ok(app)
}

/// Al cerrar la app: espera a que termine el proceso `pid`, cambia `bundle`
/// por `new_app` (guardando la vieja hasta que el cambio sale bien) y abre la
/// versión nueva.
const SWAP_SCRIPT: &str = r#"
pid="$1"; old="$2"; new="$3"; relaunch="$4"
while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
backup="$old.anterior"
rm -rf "$backup"
if mv "$old" "$backup"; then
  if mv "$new" "$old"; then
    rm -rf "$backup"
  else
    mv "$backup" "$old"
  fi
fi
xattr -dr com.apple.quarantine "$old" 2>/dev/null
[ "$relaunch" = "1" ] && open "$old"
exit 0
"#;

pub fn install_on_exit(new_app: &Path, bundle: &Path, pid: u32, relaunch: bool) -> Result<(), String> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(SWAP_SCRIPT)
        .arg("carrusel-actualizar")
        .arg(pid.to_string())
        .arg(bundle)
        .arg(new_app)
        .arg(if relaunch { "1" } else { "0" })
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("No se pudo preparar el reinicio: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compara_versiones() {
        assert_eq!(parse_version("v0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("basura", "0.1.0"));
    }

    #[test]
    fn lee_el_release_de_github() {
        let json: serde_json::Value = serde_json::from_str(r#"{
            "tag_name": "v0.2.0", "draft": false, "prerelease": false,
            "html_url": "https://github.com/JoseLMendozaV/carrusel/releases/tag/v0.2.0",
            "body": "Novedades\n---\nSHA-256 de `Carrusel-0.2.0-macOS.zip`: `AAAAe8cfb15f2df37792d93222403b6890e734c28da96312df7cc8053f6cd8eb`",
            "assets": [
                {"name": "otra-cosa.txt", "size": 1, "browser_download_url": "x"},
                {"name": "Carrusel-0.2.0-macOS.zip", "size": 41199795,
                 "browser_download_url": "https://github.com/JoseLMendozaV/carrusel/releases/download/v0.2.0/Carrusel-0.2.0-macOS.zip",
                 "digest": "sha256:3937e8cfb15f2df37792d93222403b6890e734c28da96312df7cc8053f6cd8eb"}
            ]}"#).unwrap();
        let r = parse_release(&json).unwrap();
        assert_eq!(r.version, "0.2.0");
        assert_eq!(r.asset_size, 41199795);
        assert!(r.asset_url.ends_with("Carrusel-0.2.0-macOS.zip"));
        // Manda la huella de GitHub; la de las notas es el respaldo.
        assert_eq!(r.sha256.as_deref(), Some("3937e8cfb15f2df37792d93222403b6890e734c28da96312df7cc8053f6cd8eb"));
        assert_eq!(
            sha_from_notes(&r.notes).as_deref(),
            Some("aaaae8cfb15f2df37792d93222403b6890e734c28da96312df7cc8053f6cd8eb")
        );
        let mut pre = json.clone();
        pre["prerelease"] = serde_json::Value::Bool(true);
        assert!(parse_release(&pre).is_none(), "las versiones de prueba no se ofrecen");
    }

    #[test]
    fn encuentra_el_paquete_de_la_app() {
        let exe = Path::new("/Applications/Carrusel.app/Contents/MacOS/carrusel");
        assert_eq!(bundle_of(exe), Some(PathBuf::from("/Applications/Carrusel.app")));
        assert_eq!(bundle_of(Path::new("/Users/x/carrusel/target/debug/carrusel")), None);
    }

    #[test]
    fn el_cambio_de_version_espera_y_sustituye() {
        let dir = std::env::temp_dir().join(format!("carrusel_swap_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (old, new) = (dir.join("Carrusel.app"), dir.join("descarga/Carrusel.app"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("version"), "vieja").unwrap();
        std::fs::write(new.join("version"), "nueva").unwrap();
        // Un proceso que «es la app» y se cierra al poco.
        let mut app = Command::new("sleep").arg("1").spawn().unwrap();
        install_on_exit(&new, &old, app.id(), false).unwrap();
        // Mientras la app sigue abierta no se toca nada.
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(std::fs::read_to_string(old.join("version")).unwrap(), "vieja");
        app.wait().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::fs::read_to_string(old.join("version")).ok().as_deref() != Some("nueva") {
            assert!(std::time::Instant::now() < deadline, "no se instaló la versión nueva");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        assert!(!dir.join("Carrusel.app.anterior").exists(), "la copia de respaldo se borra al terminar");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Necesita internet: `cargo test actualizacion_en_vivo -- --ignored --nocapture`
    /// Simula ser una versión antigua: descarga y verifica el último release.
    #[test]
    #[ignore]
    fn actualizacion_en_vivo() {
        let r = latest().unwrap();
        println!("último release: {} ({} bytes)", r.version, r.asset_size);
        assert!(is_newer(&r.version, "0.0.1"));
        let app = download(&r, &|f| print!("\r{:.0} %", f * 100.0)).unwrap();
        println!("\nlista para instalar: {}", app.display());
        assert_eq!(bundle_version(&app).as_deref(), Some(r.version.as_str()));
    }
}
