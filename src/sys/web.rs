//! Navegador (iPad, Safari, Chrome…): sin hilos ni disco. Las fotos viven en
//! memoria con una ruta inventada («web/<huella>/<nombre>») y se guardan
//! solas en IndexedDB; la red usa `fetch`.

use super::{MaybeSend, Request};
use image::RgbaImage;
use js_sys::{Array, Promise, Reflect, Uint8Array};
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/web/glue.js")]
extern "C" {
    #[wasm_bindgen(js_name = pick_files)]
    fn js_pick_files(accept: &str, multiple: bool) -> Promise;
    #[wasm_bindgen(js_name = download)]
    fn js_download(name: &str, bytes: &[u8], mime: &str);
    #[wasm_bindgen(js_name = can_share_files)]
    fn js_can_share_files() -> bool;
    #[wasm_bindgen(js_name = share_files)]
    fn js_share_files(files: &Array, title: &str) -> Promise;
    #[wasm_bindgen(js_name = confirm_dialog)]
    fn js_confirm(message: &str) -> bool;
    #[wasm_bindgen(js_name = decode_image)]
    fn js_decode_image(bytes: &[u8], max_pixels: f64, max_side: f64) -> Promise;
    #[wasm_bindgen(js_name = http)]
    fn js_http(method: &str, url: &str, headers: &Array, body: Option<String>) -> Promise;
    #[wasm_bindgen(js_name = sleep_ms)]
    fn js_sleep_ms(ms: f64) -> Promise;
    #[wasm_bindgen(js_name = idb_get)]
    fn js_idb_get(key: &str) -> Promise;
    #[wasm_bindgen(js_name = idb_set_text)]
    fn js_idb_set_text(key: &str, text: &str) -> Promise;
    #[wasm_bindgen(js_name = idb_set_bytes)]
    fn js_idb_set_bytes(key: &str, bytes: &[u8]) -> Promise;
    #[wasm_bindgen(js_name = idb_delete)]
    fn js_idb_delete(key: &str) -> Promise;
    #[wasm_bindgen(js_name = idb_keys)]
    fn js_idb_keys() -> Promise;
    #[wasm_bindgen(js_name = request_persistence)]
    pub fn request_persistence();
}

/// Ejecuta una tarea en el mismo hilo, entre fotograma y fotograma.
pub fn spawn(fut: impl Future<Output = ()> + MaybeSend + 'static) {
    wasm_bindgen_futures::spawn_local(fut);
}

/// Cede el turno al navegador para que pueda dibujar y atender toques
/// (entre slide y slide de una exportación, por ejemplo).
pub async fn yield_now() {
    let _ = JsFuture::from(js_sleep_ms(0.0)).await;
}

fn js_error(e: JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

// ---------------------------------------------------------------------------
// Archivos en memoria

thread_local! {
    static FILES: RefCell<HashMap<PathBuf, Arc<Vec<u8>>>> = RefCell::new(HashMap::new());
}

/// Huella FNV-1a: el mismo archivo elegido dos veces comparte ruta.
fn fingerprint(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h ^ bytes.len() as u64
}

/// Guarda un archivo elegido por el usuario y devuelve su ruta (inventada).
pub fn store_file(name: &str, bytes: Vec<u8>) -> PathBuf {
    let clean: String = name.chars().map(|c| if c == '/' || c == '\\' { '_' } else { c }).collect();
    let path = PathBuf::from(format!("web/{:016x}/{clean}", fingerprint(&bytes)));
    store_at(path.clone(), bytes);
    path
}

/// Guarda un archivo con una ruta concreta (al restaurar o abrir un proyecto).
pub fn store_at(path: PathBuf, bytes: Vec<u8>) {
    FILES.with(|f| f.borrow_mut().insert(path, Arc::new(bytes)));
}

pub fn has_file(path: &Path) -> bool {
    FILES.with(|f| f.borrow().contains_key(path))
}

pub fn file_bytes(path: &Path) -> Option<Arc<Vec<u8>>> {
    FILES.with(|f| f.borrow().get(path).cloned())
}

pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    file_bytes(path).map(|b| b.as_ref().clone()).ok_or_else(|| format!("{}: no está en este navegador", file_label(path)))
}

/// Nombre visible de un archivo (sin la carpeta inventada).
pub fn file_label(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub struct PickedFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Abre el selector de archivos. Hay que llamarla al tocar un botón (no
/// dentro de una tarea): Safari solo lo permite durante el gesto. El
/// resultado llega después; si el usuario cancela, la lista viene vacía.
pub fn pick_files(accept: &str, multiple: bool) -> impl Future<Output = Vec<PickedFile>> + 'static {
    let promise = js_pick_files(accept, multiple);
    async move {
        let Ok(list) = JsFuture::from(promise).await else { return Vec::new() };
        Array::from(&list)
            .iter()
            .filter_map(|item| {
                let name = Reflect::get(&item, &"name".into()).ok()?.as_string()?;
                let bytes = Uint8Array::new(&Reflect::get(&item, &"bytes".into()).ok()?).to_vec();
                Some(PickedFile { name, bytes })
            })
            .collect()
    }
}

pub fn download(name: &str, bytes: &[u8], mime: &str) {
    js_download(name, bytes, mime);
}

pub fn can_share_files() -> bool {
    js_can_share_files()
}

/// Abre la hoja de compartir del sistema (en iPad: «Guardar N imágenes»,
/// Instagram, AirDrop…). Hay que llamarla al tocar un botón. Devuelve
/// `Ok(true)` si se compartió, `Ok(false)` si se canceló.
pub fn share_files(files: &[(String, Vec<u8>, &'static str)], title: &str) -> impl Future<Output = Result<bool, String>> + 'static {
    let list = Array::new();
    for (name, bytes, mime) in files {
        let o = js_sys::Object::new();
        let _ = Reflect::set(&o, &"name".into(), &name.into());
        let _ = Reflect::set(&o, &"mime".into(), &(*mime).into());
        let _ = Reflect::set(&o, &"bytes".into(), &Uint8Array::from(bytes.as_slice()));
        list.push(&o);
    }
    let promise = js_share_files(&list, title);
    async move {
        let r = JsFuture::from(promise).await.map_err(js_error)?.as_string().unwrap_or_default();
        match r.as_str() {
            "ok" => Ok(true),
            "cancel" => Ok(false),
            other => Err(other.trim_start_matches("error: ").to_string()),
        }
    }
}

pub fn confirm(message: &str) -> bool {
    js_confirm(message)
}

/// Título de la pestaña (solo se toca el documento si cambia).
pub fn set_title(title: &str) {
    thread_local! {
        static LAST: RefCell<String> = RefCell::new(String::new());
    }
    LAST.with(|last| {
        if *last.borrow() != title {
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                doc.set_title(title);
            }
            *last.borrow_mut() = title.to_string();
        }
    });
}

/// Lado máximo y píxeles máximos de una foto en memoria: de sobra para
/// exportar a 1080 px y dentro de los límites de Safari en iPad.
const MAX_SIDE: f64 = 8192.0;
const MAX_PIXELS: f64 = 16_000_000.0;

pub async fn load_image(path: &Path) -> Result<RgbaImage, String> {
    let bytes = file_bytes(path).ok_or_else(|| format!("{}: no está en este navegador", file_label(path)))?;
    // Primero el decodificador del navegador; si no puede, el de Carrusel.
    if let Ok(v) = JsFuture::from(js_decode_image(&bytes[..], MAX_PIXELS, MAX_SIDE)).await {
        if !v.is_null() {
            let w = Reflect::get(&v, &"width".into()).ok().and_then(|x| x.as_f64()).unwrap_or(0.0) as u32;
            let h = Reflect::get(&v, &"height".into()).ok().and_then(|x| x.as_f64()).unwrap_or(0.0) as u32;
            let pixels = Reflect::get(&v, &"pixels".into()).map(|p| Uint8Array::new(&p).to_vec()).unwrap_or_default();
            if let Some(img) = RgbaImage::from_raw(w, h, pixels) {
                return Ok(img);
            }
        }
    }
    let img = crate::render::decode_image(&bytes).map_err(|e| format!("{}: {e}", file_label(path)))?;
    let s = (MAX_SIDE as f32 / img.width().max(img.height()) as f32).min(1.0);
    if s < 1.0 {
        let (w, h) = (((img.width() as f32 * s) as u32).max(1), ((img.height() as f32 * s) as u32).max(1));
        return Ok(image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle));
    }
    Ok(img)
}

// ---------------------------------------------------------------------------
// Red

pub async fn http(req: Request<'_>) -> Result<Vec<u8>, String> {
    let headers = Array::new();
    for (name, value) in &req.headers {
        headers.push(&(*name).into());
        headers.push(&value.into());
    }
    let promise = js_http(req.method, req.url, &headers, req.body);
    let bytes = JsFuture::from(promise).await.map_err(js_error)?;
    Ok(Uint8Array::new(&bytes).to_vec())
}

// ---------------------------------------------------------------------------
// Caché y ajustes

thread_local! {
    static CACHE: RefCell<HashMap<String, Vec<u8>>> = RefCell::new(HashMap::new());
}

/// Caché en memoria (el navegador ya guarda las respuestas de la red).
pub fn cache_get(key: &str, _max_age_days: Option<u64>) -> Option<Vec<u8>> {
    CACHE.with(|c| c.borrow().get(key).cloned())
}

pub fn cache_put(key: &str, data: &[u8]) {
    CACHE.with(|c| c.borrow_mut().insert(key.to_string(), data.to_vec()));
}

/// Rutas de mentira para el código de escritorio que se compila igual
/// (actualizaciones): en la web cualquier acceso al disco falla sin más.
pub fn cache_dir() -> PathBuf {
    PathBuf::from("/sin-disco/cache")
}

pub fn data_dir() -> PathBuf {
    PathBuf::from("/sin-disco/datos")
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

pub fn setting_get(key: &str) -> Option<String> {
    local_storage()?.get_item(&format!("carrusel.{key}")).ok()?.filter(|t| !t.trim().is_empty())
}

pub fn setting_set(key: &str, value: &str) -> Result<(), String> {
    local_storage()
        .ok_or("El navegador no deja guardar ajustes.")?
        .set_item(&format!("carrusel.{key}"), value)
        .map_err(js_error)
}

pub fn setting_remove(key: &str) {
    if let Some(s) = local_storage() {
        let _ = s.remove_item(&format!("carrusel.{key}"));
    }
}

// ---------------------------------------------------------------------------
// IndexedDB: guardado automático del proyecto

pub async fn db_get_text(key: &str) -> Option<String> {
    JsFuture::from(js_idb_get(key)).await.ok()?.as_string()
}

pub async fn db_get_bytes(key: &str) -> Option<Vec<u8>> {
    let v = JsFuture::from(js_idb_get(key)).await.ok()?;
    v.is_instance_of::<Uint8Array>().then(|| Uint8Array::new(&v).to_vec())
}

pub fn db_set_text(key: &str, text: &str) -> impl Future<Output = bool> + 'static {
    let p = js_idb_set_text(key, text);
    async move { JsFuture::from(p).await.ok().and_then(|v| v.as_bool()).unwrap_or(false) }
}

pub fn db_set_bytes(key: &str, bytes: &[u8]) -> impl Future<Output = bool> + 'static {
    let p = js_idb_set_bytes(key, bytes);
    async move { JsFuture::from(p).await.ok().and_then(|v| v.as_bool()).unwrap_or(false) }
}

pub fn db_delete(key: &str) -> impl Future<Output = ()> + 'static {
    let p = js_idb_delete(key);
    async move {
        let _ = JsFuture::from(p).await;
    }
}

pub async fn db_keys() -> Vec<String> {
    match JsFuture::from(js_idb_keys()).await {
        Ok(v) => Array::from(&v).iter().filter_map(|k| k.as_string()).collect(),
        Err(_) => Vec::new(),
    }
}
