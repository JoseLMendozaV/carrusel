//! Versión web (pensada para iPad): arranque, fotos elegidas o soltadas,
//! guardado automático en el navegador, abrir y guardar proyectos como un
//! único archivo con sus fotos, y exportar para compartir (en iPad, «Guardar
//! imágenes» en Fotos) o descargar en un .zip.

use crate::doc::{Doc, Kind};
use crate::render::{self, ExportFormat};
use crate::{sys, App, ExportMsg, Place, ACCENT};
use base64::Engine;
use eframe::egui::{self, Color32};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use wasm_bindgen::JsCast;

/// Arranca la app en el `<canvas id="carrusel">` de `index.html`.
pub fn start() {
    eframe::WebLogger::init(log::LevelFilter::Warn).ok();
    sys::request_persistence();
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().and_then(|w| w.document()).expect("sin documento");
        let canvas = document
            .get_element_by_id("carrusel")
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("falta <canvas id=\"carrusel\">");
        let result = eframe::WebRunner::new()
            .start(canvas, eframe::WebOptions::default(), Box::new(|cc| Ok(Box::new(App::new(cc)))))
            .await;
        if let Some(loading) = document.get_element_by_id("cargando") {
            match result {
                Ok(()) => loading.remove(),
                Err(e) => {
                    log::error!("No se pudo iniciar Carrusel: {e:?}");
                    loading.set_inner_html(
                        "<p>No se pudo iniciar Carrusel en este navegador.<br>Actualiza Safari (o iPadOS) y vuelve a intentarlo.</p>",
                    );
                }
            }
        }
    });
}

/// Algo que una tarea en segundo plano quiere hacer con la app al terminar.
type Job = Box<dyn FnOnce(&mut App, &egui::Context)>;

/// Archivos exportados listos para compartir o descargar.
struct Export {
    files: Vec<(String, Vec<u8>, &'static str)>,
    name: String,
}

pub struct WebState {
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<Job>,
    ready: Option<Export>,
    can_share: bool,
    /// El proyecto guardado ya se recuperó (antes no se guarda nada encima).
    restored: bool,
    /// Último proyecto guardado (JSON) y fotos ya guardadas en el navegador.
    saved_json: String,
    saved_files: HashSet<PathBuf>,
    next_autosave: f64,
    autosave_scheduled: bool,
    layout_done: bool,
}

impl Default for WebState {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            ready: None,
            can_share: false,
            restored: false,
            saved_json: String::new(),
            saved_files: HashSet::new(),
            next_autosave: 0.0,
            autosave_scheduled: false,
            layout_done: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Guardado automático (IndexedDB)

const DOC_KEY: &str = "proyecto";
const FILE_PREFIX: &str = "archivo:";

fn file_key(path: &Path) -> String {
    format!("{FILE_PREFIX}{}", path.to_string_lossy())
}

#[derive(Serialize)]
struct SavedRef<'a> {
    name: Option<&'a str>,
    dirty: bool,
    doc: &'a Doc,
}

#[derive(Deserialize)]
struct Saved {
    name: Option<String>,
    #[serde(default)]
    dirty: bool,
    doc: Doc,
}

/// Fotos, videos y fuente que usa el documento.
fn doc_files(doc: &Doc) -> Vec<PathBuf> {
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

/// Recupera el último proyecto y borra del navegador las fotos que ya no usa.
async fn restore() -> Option<Saved> {
    let saved: Option<Saved> = sys::db_get_text(DOC_KEY).await.and_then(|t| serde_json::from_str(&t).ok());
    let wanted: HashSet<PathBuf> = saved.as_ref().map(|s| doc_files(&s.doc).into_iter().collect()).unwrap_or_default();
    for p in &wanted {
        if let Some(bytes) = sys::db_get_bytes(&file_key(p)).await {
            sys::store_at(p.clone(), bytes);
        }
    }
    for key in sys::db_keys().await {
        if key.strip_prefix(FILE_PREFIX).is_some_and(|p| !wanted.contains(Path::new(p))) {
            sys::db_delete(&key).await;
        }
    }
    saved
}

impl App {
    fn send_job(tx: &mpsc::Sender<Job>, ctx: &egui::Context, job: impl FnOnce(&mut App, &egui::Context) + 'static) {
        let _ = tx.send(Box::new(job));
        ctx.request_repaint();
    }

    pub(crate) fn web_init(&mut self, ctx: &egui::Context) {
        self.web.can_share = sys::can_share_files();
        let (tx, ctx) = (self.web.tx.clone(), ctx.clone());
        sys::spawn(async move {
            let saved = restore().await;
            Self::send_job(&tx, &ctx, move |app, ctx| app.finish_restore(saved, ctx));
        });
    }

    fn finish_restore(&mut self, saved: Option<Saved>, ctx: &egui::Context) {
        self.web.restored = true;
        // Si ya empezaste a trabajar mientras tanto, no se pisa.
        let untouched = self.doc.layers.is_empty() && self.undo.is_empty();
        if let Some(s) = saved.filter(|_| untouched) {
            self.web.saved_files = doc_files(&s.doc).into_iter().filter(|p| sys::has_file(p)).collect();
            let has_content = !s.doc.layers.is_empty();
            self.set_doc(s.doc, s.name.map(PathBuf::from), ctx);
            self.undo.clear();
            self.redo.clear();
            self.dirty = s.dirty;
            if has_content {
                self.status = "Se recuperó tu último proyecto.".into();
            }
        }
        self.web.saved_json = self.autosave_json();
        // Para entonces los paneles ya tienen su tamaño final.
        self.fit_request = true;
    }

    fn autosave_json(&self) -> String {
        let name = self.path.as_ref().and_then(|p| p.to_str());
        serde_json::to_string(&SavedRef { name, dirty: self.dirty, doc: &self.doc }).unwrap_or_default()
    }

    /// Guarda el proyecto en el navegador si cambió (como mucho una vez por segundo).
    fn autosave(&mut self) {
        let json = self.autosave_json();
        if json == self.web.saved_json {
            return;
        }
        // Primero las fotos nuevas, luego el documento que las usa (IndexedDB
        // respeta el orden de las escrituras).
        for p in doc_files(&self.doc) {
            if self.web.saved_files.contains(&p) {
                continue;
            }
            if let Some(bytes) = sys::file_bytes(&p) {
                let write = sys::db_set_bytes(&file_key(&p), &bytes);
                sys::spawn(async move {
                    write.await;
                });
                self.web.saved_files.insert(p);
            }
        }
        let write = sys::db_set_text(DOC_KEY, &json);
        sys::spawn(async move {
            write.await;
        });
        self.web.saved_json = json;
    }

    /// Lo que la versión web hace en cada fotograma.
    pub(crate) fn web_frame(&mut self, ctx: &egui::Context) {
        while let Ok(job) = self.web.rx.try_recv() {
            job(self, ctx);
        }
        if !self.web.layout_done {
            self.web.layout_done = true;
            // En un iPad en vertical la biblioteca quitaría casi todo el lienzo.
            if ctx.content_rect().width() < 1000.0 {
                self.library.open = false;
            }
        }
        if !self.web.restored {
            return;
        }
        let now = ctx.input(|i| i.time);
        if now >= self.web.next_autosave {
            if self.drag.is_none() {
                self.autosave();
            }
            self.web.next_autosave = now + 1.0;
            self.web.autosave_scheduled = false;
        } else if !self.web.autosave_scheduled {
            // Un fotograma más dentro de un rato para guardar el último cambio.
            self.web.autosave_scheduled = true;
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(self.web.next_autosave - now + 0.05));
        }
    }

    // -----------------------------------------------------------------------
    // Fotos, fuentes y proyectos

    pub(crate) fn web_pick_media(&mut self, ctx: &egui::Context) {
        // «image/*» abre la fototeca del iPad (y convierte HEIC a JPG).
        let picked = sys::pick_files("image/*", true);
        let (tx, ctx) = (self.web.tx.clone(), ctx.clone());
        sys::spawn(async move {
            let files = picked.await;
            if files.is_empty() {
                return;
            }
            Self::send_job(&tx, &ctx, move |app, ctx| {
                let paths = files.into_iter().map(|f| sys::store_file(&f.name, f.bytes)).collect();
                app.add_media(paths, ctx);
            });
        });
    }

    /// Fotos arrastradas a Safari (por ejemplo desde Fotos en Split View).
    pub(crate) fn web_drop(&mut self, files: Vec<egui::DroppedFileHandle>, place: Option<Place>, ctx: &egui::Context) {
        let (tx, ctx) = (self.web.tx.clone(), ctx.clone());
        sys::spawn(async move {
            let mut read = Vec::new();
            for f in &files {
                if let Ok(bytes) = f.bytes_async().await {
                    read.push((sys::file_label(f.path()), bytes));
                }
            }
            Self::send_job(&tx, &ctx, move |app, ctx| {
                let paths: Vec<PathBuf> = read.into_iter().map(|(name, bytes)| sys::store_file(&name, bytes)).collect();
                match place {
                    Some(place) if paths.len() == 1 => app.request_load(paths[0].clone(), place, ctx),
                    _ => app.add_media(paths, ctx),
                }
            });
        });
    }

    pub(crate) fn web_pick_font(&mut self, ctx: &egui::Context) {
        // Sin filtro: iPadOS no siempre reconoce los tipos de fuente.
        let picked = sys::pick_files("", false);
        let (tx, ctx) = (self.web.tx.clone(), ctx.clone());
        sys::spawn(async move {
            let Some(f) = picked.await.into_iter().next() else { return };
            Self::send_job(&tx, &ctx, move |app, ctx| {
                if ab_glyph::FontArc::try_from_vec(f.bytes.clone()).is_err() {
                    app.status = format!("«{}» no es una fuente .ttf, .otf o .ttc.", f.name);
                    return;
                }
                let path = sys::store_file(&f.name, f.bytes);
                app.checkpoint();
                app.doc.font = Some(path);
                app.sync_font(ctx);
            });
        });
    }

    pub(crate) fn web_open(&mut self, ctx: &egui::Context) {
        let picked = sys::pick_files("", false);
        let (tx, ctx) = (self.web.tx.clone(), ctx.clone());
        sys::spawn(async move {
            let Some(f) = picked.await.into_iter().next() else { return };
            Self::send_job(&tx, &ctx, move |app, ctx| app.open_project_file(&f.name, &f.bytes, ctx));
        });
    }

    fn open_project_file(&mut self, name: &str, bytes: &[u8], ctx: &egui::Context) {
        let (doc, files) = match parse_project(bytes) {
            Ok(r) => r,
            Err(e) => {
                self.status = format!("No se pudo abrir «{name}»: {e}");
                return;
            }
        };
        for (path, data) in files {
            sys::store_at(path, data);
        }
        let missing = doc_files(&doc).iter().filter(|p| !sys::has_file(p)).count();
        self.web.saved_files.clear();
        self.set_doc(doc, Some(PathBuf::from(name)), ctx);
        self.status = if missing > 0 {
            format!("Abierto «{name}», pero faltan {missing} archivo(s): el proyecto se guardó en la Mac sin sus fotos.")
        } else {
            format!("Abierto «{name}».")
        };
    }

    /// «Guardar» descarga el proyecto con sus fotos en un solo archivo.
    pub(crate) fn web_save(&mut self) {
        let mut files = BTreeMap::new();
        for p in doc_files(&self.doc) {
            if let Some(bytes) = sys::file_bytes(&p) {
                files.insert(p.to_string_lossy().into_owned(), base64::engine::general_purpose::STANDARD.encode(&bytes[..]));
            }
        }
        let bundle = BundleRef { carrusel: BUNDLE_VERSION, doc: &self.doc, files };
        let json = match serde_json::to_vec(&bundle) {
            Ok(j) => j,
            Err(e) => {
                self.status = format!("No se pudo guardar: {e}");
                return;
            }
        };
        let name = format!("{}.carrusel", self.doc_name());
        sys::download(&name, &json, "application/octet-stream");
        self.path = Some(PathBuf::from(&name));
        self.dirty = false;
        self.status = format!("Proyecto descargado: «{name}» (en iPad queda en Archivos › Descargas).");
    }

    // -----------------------------------------------------------------------
    // Exportar

    pub(crate) fn web_export(&mut self, ctx: &egui::Context) {
        self.web.ready = None;
        let doc = self.doc.clone();
        let images: HashMap<PathBuf, Arc<RgbaImage>> = self.images.iter().map(|(k, v)| (k.clone(), v.full.clone())).collect();
        let font = self.font.clone();
        let (format, panorama, name) = (self.export_format, self.export_panorama, self.doc_name());
        let (tx, rx) = mpsc::channel();
        self.exporting = Some(rx);
        self.status = "Exportando…".into();
        let (jobs, ctx) = (self.web.tx.clone(), ctx.clone());
        sys::spawn(async move {
            let progress = |f: f32| {
                let _ = tx.send(ExportMsg::Progress(f));
                ctx.request_repaint();
            };
            let result = export_files(&doc, &images, &font, format, panorama, &progress).await;
            let msg = result.map(|files| {
                let n = files.len();
                Self::send_job(&jobs, &ctx, move |app, _| app.web.ready = Some(Export { files, name }));
                if sys::can_share_files() {
                    format!("✔ {n} archivo(s) listos: toca «Guardar en Fotos / compartir».")
                } else {
                    format!("✔ {n} archivo(s) listos: toca «Descargar .zip».")
                }
            });
            let _ = tx.send(ExportMsg::Done(msg));
            ctx.request_repaint();
        });
    }

    /// Barra con la exportación lista: compartir (Fotos, Instagram, AirDrop…) o descargar.
    pub(crate) fn web_banner(&mut self, ui: &mut egui::Ui) {
        let Some(export) = &self.web.ready else { return };
        let ctx = ui.ctx().clone();
        let mut close = false;
        egui::Panel::top("export_ready")
            .frame(egui::Frame::new().fill(Color32::from_rgb(36, 52, 44)).inner_margin(egui::Margin::symmetric(10, 8)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(format!("✔ {} archivo(s) exportados.", export.files.len())).strong());
                    if self.web.can_share {
                        let share = egui::Button::new(egui::RichText::new("Guardar en Fotos / compartir").strong().color(Color32::WHITE)).fill(ACCENT);
                        if ui.add(share).on_hover_text("Abre la hoja de compartir: «Guardar imágenes», Instagram, AirDrop…").clicked() {
                            let title = export.name.clone();
                            let sharing = sys::share_files(&export.files, &title);
                            let tx = self.web.tx.clone();
                            let ctx = ctx.clone();
                            sys::spawn(async move {
                                let result = sharing.await;
                                Self::send_job(&tx, &ctx, move |app, _| match result {
                                    Ok(true) => app.status = "Listo.".into(),
                                    Ok(false) => {}
                                    Err(e) => app.status = format!("No se pudo compartir ({e}). Prueba «Descargar .zip»."),
                                });
                            });
                        }
                    }
                    if ui.button("Descargar .zip").clicked() {
                        let zip = zip_store(&export.files);
                        sys::download(&format!("{}-slides.zip", export.name), &zip, "application/zip");
                    }
                    if ui.button("Cerrar").clicked() {
                        close = true;
                    }
                });
            });
        if close {
            self.web.ready = None;
        }
    }
}

/// Dibuja cada slide (y el panorama si se pide) y lo codifica, cediendo el
/// turno al navegador entre uno y otro para que la app no se congele.
async fn export_files(
    doc: &Doc,
    images: &HashMap<PathBuf, Arc<RgbaImage>>,
    font: &ab_glyph::FontArc,
    format: ExportFormat,
    panorama: bool,
    progress: &dyn Fn(f32),
) -> Result<Vec<(String, Vec<u8>, &'static str)>, String> {
    let kind = |f: ExportFormat| match f {
        ExportFormat::Jpg => ("jpg", "image/jpeg"),
        ExportFormat::Png => ("png", "image/png"),
    };
    let (sw, _) = doc.format.size();
    let total = (doc.slides + panorama as u32) as f32;
    let mut files = Vec::new();
    for i in 0..doc.slides {
        sys::yield_now().await;
        let img = render::render_region(doc, images, font, (i * sw) as f32, sw);
        let (ext, mime) = kind(format);
        files.push((render::slide_name(i, doc.slides, ext), render::encode_image(img, format)?, mime));
        progress((i + 1) as f32 / total);
    }
    if panorama {
        sys::yield_now().await;
        let width = doc.slides * sw;
        // Un JPG no puede medir más de 65 535 px de ancho.
        let format = if format == ExportFormat::Jpg && width > 65_535 { ExportFormat::Png } else { format };
        let img = render::render_region(doc, images, font, 0.0, width);
        let (ext, mime) = kind(format);
        files.push((format!("panorama.{ext}"), render::encode_image(img, format)?, mime));
        progress(1.0);
    }
    Ok(files)
}

// ---------------------------------------------------------------------------
// Proyecto en un solo archivo: el documento y sus fotos (en base64)

const BUNDLE_VERSION: u32 = 1;

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

/// Lee un proyecto: el de la versión web (con fotos) o uno de la Mac (solo
/// el documento, con rutas a fotos que aquí no existen).
fn parse_project(bytes: &[u8]) -> Result<(Doc, Vec<(PathBuf, Vec<u8>)>), String> {
    if let Ok(b) = serde_json::from_slice::<Bundle>(bytes) {
        if b.carrusel > BUNDLE_VERSION {
            return Err("es de una versión más nueva de Carrusel.".into());
        }
        let files = b
            .files
            .into_iter()
            .filter_map(|(p, data)| base64::engine::general_purpose::STANDARD.decode(data).ok().map(|d| (PathBuf::from(p), d)))
            .collect();
        return Ok((b.doc, files));
    }
    serde_json::from_slice::<Doc>(bytes).map(|d| (d, Vec::new())).map_err(|_| "no es un proyecto de Carrusel.".to_string())
}

// ---------------------------------------------------------------------------
// .zip sin comprimir (las fotos ya vienen comprimidas)

fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *v = c;
        }
        t
    });
    !data.iter().fold(!0u32, |c, &b| table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8))
}

fn zip_store(files: &[(String, Vec<u8>, &'static str)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    // 1 de enero de 1980 a medianoche, en formato MS-DOS.
    let (time, date) = (0u16, 0x21u16);
    for (name, data, _) in files {
        let (crc, size, offset, name) = (crc32(data), data.len() as u32, out.len() as u32, name.as_bytes());
        out.extend(0x0403_4b50u32.to_le_bytes());
        for v in [20u16, 0x0800, 0, time, date] {
            out.extend(v.to_le_bytes()); // versión, UTF-8, sin compresión, hora, fecha
        }
        for v in [crc, size, size] {
            out.extend(v.to_le_bytes());
        }
        out.extend((name.len() as u16).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(name);
        out.extend(data);

        central.extend(0x0201_4b50u32.to_le_bytes());
        for v in [20u16, 20, 0x0800, 0, time, date] {
            central.extend(v.to_le_bytes());
        }
        for v in [crc, size, size] {
            central.extend(v.to_le_bytes());
        }
        for v in [name.len() as u16, 0, 0, 0, 0] {
            central.extend(v.to_le_bytes()); // nombre, extra, comentario, disco, atributos internos
        }
        central.extend(0u32.to_le_bytes()); // atributos externos
        central.extend(offset.to_le_bytes());
        central.extend(name);
    }
    let (cd_offset, cd_size, count) = (out.len() as u32, central.len() as u32, files.len() as u16);
    out.extend(central);
    out.extend(0x0605_4b50u32.to_le_bytes());
    for v in [0u16, 0, count, count] {
        out.extend(v.to_le_bytes());
    }
    out.extend(cd_size.to_le_bytes());
    out.extend(cd_offset.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}
