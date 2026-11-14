//! Proyecto `.carrusel` con las fotos dentro: un solo archivo que se abre
//! igual en la Mac y en la versión web (iPad). Es el mismo JSON del
//! documento, envuelto junto a sus fotos y su fuente en base64.
//!
//! Los proyectos antiguos de la Mac (solo el documento, con rutas a las
//! fotos) se siguen abriendo.

use crate::doc::{Doc, Kind};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const BUNDLE_VERSION: u32 = 1;

#[derive(Serialize)]
struct BundleRef<'a> {
    carrusel: u32,
    doc: &'a Doc,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct Bundle {
    carrusel: u32,
    doc: Doc,
    #[serde(default)]
    files: BTreeMap<String, String>,
}

/// Fotos, videos y fuente que usa el documento (sin repetir).
pub fn doc_files(doc: &Doc) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = doc
        .layers
        .iter()
        .filter_map(|l| match &l.kind {
            Kind::Image { path } | Kind::Video { path, .. } => Some(path.clone()),
            _ => None,
        })
        .collect();
    out.extend(doc.font.clone());
    out.sort();
    out.dedup();
    out
}

/// Cambia las rutas de fotos, videos y fuente según `map`.
pub fn remap_files(doc: &mut Doc, map: &impl Fn(&Path) -> Option<PathBuf>) {
    for l in &mut doc.layers {
        if let Kind::Image { path } | Kind::Video { path, .. } = &mut l.kind {
            if let Some(p) = map(path) {
                *path = p;
            }
        }
    }
    if let Some(p) = doc.font.as_deref().and_then(map) {
        doc.font = Some(p);
    }
}

/// Lo que no se pudo meter en el archivo.
#[derive(Default)]
pub struct Left {
    /// Archivos que no se encontraron.
    pub missing: Vec<PathBuf>,
    /// Videos que se dejaron fuera (ocupan demasiado y la web aún no los usa).
    pub videos: Vec<PathBuf>,
}

/// Empaqueta el documento con sus archivos. `read` devuelve el contenido de
/// cada uno; los videos se dejan fuera.
pub fn bundle(doc: &Doc, read: impl Fn(&Path) -> Option<Vec<u8>>) -> Result<(Vec<u8>, Left), String> {
    let videos: Vec<PathBuf> = doc
        .layers
        .iter()
        .filter_map(|l| match &l.kind {
            Kind::Video { path, .. } => Some(path.clone()),
            _ => None,
        })
        .collect();
    let mut left = Left::default();
    let mut files = BTreeMap::new();
    for p in doc_files(doc) {
        if videos.contains(&p) {
            left.videos.push(p);
        } else if let Some(bytes) = read(&p) {
            files.insert(p.to_string_lossy().into_owned(), base64::engine::general_purpose::STANDARD.encode(bytes));
        } else {
            left.missing.push(p);
        }
    }
    let json = serde_json::to_vec(&BundleRef { carrusel: BUNDLE_VERSION, doc, files }).map_err(|e| e.to_string())?;
    Ok((json, left))
}

pub struct Opened {
    pub doc: Doc,
    /// Archivos que venían dentro, con la ruta que tienen en el documento.
    pub files: Vec<(PathBuf, Vec<u8>)>,
    /// Era un proyecto con fotos dentro (y no uno antiguo con rutas).
    pub bundle: bool,
}

pub fn parse(bytes: &[u8]) -> Result<Opened, String> {
    if let Ok(b) = serde_json::from_slice::<Bundle>(bytes) {
        if b.carrusel > BUNDLE_VERSION {
            return Err("es de una versión más nueva de Carrusel.".into());
        }
        let files = b
            .files
            .into_iter()
            .filter_map(|(p, data)| base64::engine::general_purpose::STANDARD.decode(data).ok().map(|d| (PathBuf::from(p), d)))
            .collect();
        return Ok(Opened { doc: b.doc, files, bundle: true });
    }
    serde_json::from_slice::<Doc>(bytes)
        .map(|doc| Opened { doc, files: Vec::new(), bundle: false })
        .map_err(|_| "no es un proyecto de Carrusel.".to_string())
}

/// Huella FNV-1a de un archivo (para nombres que no choquen).
pub fn fingerprint(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^ bytes.len() as u64
}

/// Nombre con el que se guarda en disco un archivo que venía dentro del
/// proyecto (las fotos del iPad suelen llamarse todas «image.jpg»).
pub fn unpacked_name(path: &Path, bytes: &[u8]) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "archivo".into());
    format!("{:08x}-{name}", fingerprint(bytes) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::Layer;

    #[test]
    fn ida_y_vuelta_con_fotos_dentro() {
        let mut doc = Doc::default();
        doc.layers.push(Layer::new(1, Kind::Image { path: "/fotos/a.jpg".into() }, (0.0, 0.0, 10.0, 10.0)));
        doc.layers.push(Layer::new(2, Kind::Video { path: "/fotos/v.mov".into(), start: 0.0, muted: false }, (0.0, 0.0, 10.0, 10.0)));
        doc.layers.push(Layer::new(3, Kind::Image { path: "/fotos/falta.jpg".into() }, (0.0, 0.0, 10.0, 10.0)));
        doc.font = Some("/fuentes/f.ttf".into());
        let read = |p: &Path| match p.to_str() {
            Some("/fotos/a.jpg") => Some(vec![1, 2, 3]),
            Some("/fuentes/f.ttf") => Some(vec![9]),
            _ => None,
        };
        let (bytes, left) = bundle(&doc, read).unwrap();
        assert_eq!(left.videos, vec![PathBuf::from("/fotos/v.mov")]);
        assert_eq!(left.missing, vec![PathBuf::from("/fotos/falta.jpg")]);
        let opened = parse(&bytes).unwrap();
        assert!(opened.bundle);
        assert_eq!(opened.doc.layers.len(), 3);
        let mut files = opened.files;
        files.sort();
        assert_eq!(files, vec![(PathBuf::from("/fotos/a.jpg"), vec![1, 2, 3]), (PathBuf::from("/fuentes/f.ttf"), vec![9])]);
    }

    #[test]
    fn abre_proyectos_antiguos_con_rutas() {
        let json = serde_json::to_vec(&Doc::default()).unwrap();
        let opened = parse(&json).unwrap();
        assert!(!opened.bundle && opened.files.is_empty());
        assert!(parse(b"hola").is_err());
    }

    #[test]
    fn cambia_rutas() {
        let mut doc = Doc::default();
        doc.layers.push(Layer::new(1, Kind::Image { path: "web/abc/image.jpg".into() }, (0.0, 0.0, 1.0, 1.0)));
        doc.font = Some("web/def/f.ttf".into());
        remap_files(&mut doc, &|p: &Path| Some(Path::new("/nuevo").join(p.file_name().unwrap())));
        assert_eq!(doc_files(&doc), vec![PathBuf::from("/nuevo/f.ttf"), PathBuf::from("/nuevo/image.jpg")]);
        assert_ne!(unpacked_name(Path::new("a/image.jpg"), b"x"), unpacked_name(Path::new("b/image.jpg"), b"y"));
    }
}
