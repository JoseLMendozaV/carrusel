//! Versión web (pensada para iPad): arranque, fotos elegidas o soltadas,
//! guardado automático en el navegador, abrir y guardar proyectos como un
//! único archivo con sus fotos, y exportar para compartir (en iPad, «Guardar
//! imágenes» en Fotos) o descargar en un .zip.

use crate::doc::Doc;
use crate::render::{self, ExportFormat};
use crate::{project, sys, App, ExportMsg, Place, ACCENT};
use eframe::egui::{self, Color32, TextureHandle, Vec2};
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    /// Ya se leyó la lista de proyectos (antes no se guarda nada encima).
    restored: bool,
    /// Proyecto abierto y lista de proyectos (el más reciente primero).
    current: String,
    projects: Vec<Entry>,
    show_projects: bool,
    /// Proyecto que se está renombrando y el texto nuevo.
    renaming: Option<(String, String)>,
    /// Último proyecto guardado (JSON) y fotos ya guardadas en el navegador.
    saved_json: String,
    saved_files: HashSet<PathBuf>,
    /// Huella del proyecto cuando se hizo su miniatura.
    thumb_for: u64,
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
            current: String::new(),
            projects: Vec::new(),
            show_projects: false,
            renaming: None,
            saved_json: String::new(),
            saved_files: HashSet::new(),
            thumb_for: 0,
            next_autosave: 0.0,
            autosave_scheduled: false,
            layout_done: false,
        }
    }
}

fn hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// Fecha y hora legibles («14/11/2026 11:30»).
fn format_date(ms: f64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms));
    format!("{}/{}/{} {:02}:{:02}", d.get_date(), d.get_month() + 1, d.get_full_year(), d.get_hours(), d.get_minutes())
}

// ---------------------------------------------------------------------------
// Proyectos guardados en el navegador (IndexedDB)
//
//   proyecto:<id>   el documento (JSON)
//   info:<id>       nombre, fecha y archivos que usa (JSON, para la lista)
//   miniatura:<id>  PNG pequeño de los primeros slides
//   archivo:<ruta>  fotos y fuentes, compartidas entre proyectos
//   actual          el proyecto abierto

const LEGACY_KEY: &str = "proyecto";
const CURRENT_KEY: &str = "actual";
const FILE_PREFIX: &str = "archivo:";
const INFO_PREFIX: &str = "info:";

fn file_key(path: &Path) -> String {
    format!("{FILE_PREFIX}{}", path.to_string_lossy())
}
fn doc_key(id: &str) -> String {
    format!("proyecto:{id}")
}
fn info_key(id: &str) -> String {
    format!("{INFO_PREFIX}{id}")
}
fn thumb_key(id: &str) -> String {
    format!("miniatura:{id}")
}

fn new_id() -> String {
    thread_local! {
        static COUNTER: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }
    let n = COUNTER.with(|c| {
        c.set(c.get() + 1);
        c.get()
    });
    format!("p{:x}{n:x}", sys::now_ms() as u64)
}

#[derive(Serialize)]
struct SavedRef<'a> {
    dirty: bool,
    doc: &'a Doc,
}

#[derive(Deserialize)]
struct Saved {
    /// Solo en proyectos guardados por la primera versión web.
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    dirty: bool,
    doc: Doc,
}

#[derive(Clone, Serialize, Deserialize)]
struct Info {
    name: String,
    /// Milisegundos desde 1970.
    modified: f64,
    #[serde(default)]
    files: Vec<String>,
}

struct Entry {
    id: String,
    info: Info,
    thumb_png: Option<Vec<u8>>,
    thumb: Option<TextureHandle>,
}

fn file_list(doc: &Doc) -> Vec<String> {
    project::doc_files(doc).iter().map(|p| p.to_string_lossy().into_owned()).collect()
}

/// Nombre válido para un proyecto (también es el nombre del archivo al guardarlo).
fn clean_name(name: &str) -> String {
    let n: String = name.trim().chars().map(|c| if matches!(c, '/' | '\\' | ':') { '-' } else { c }).collect();
    if n.is_empty() { "Sin título".into() } else { n }
}

fn spawn_write(write: impl std::future::Future<Output = bool> + 'static) {
    sys::spawn(async move {
        write.await;
    });
}

/// Lista de proyectos (la más reciente primero) y el que estaba abierto. La
/// primera vez convierte el proyecto único de la versión anterior.
async fn load_index() -> (Vec<Entry>, Option<String>) {
    if let Some(text) = sys::db_get_text(LEGACY_KEY).await {
        if let Ok(saved) = serde_json::from_str::<Saved>(&text) {
            let id = new_id();
            let name = saved.name.as_deref().and_then(|n| Path::new(n).file_stem()).map(|s| s.to_string_lossy().into_owned());
            let info = Info { name: clean_name(name.as_deref().unwrap_or("")), modified: sys::now_ms(), files: file_list(&saved.doc) };
            sys::db_set_text(&doc_key(&id), &text).await;
            sys::db_set_text(&info_key(&id), &serde_json::to_string(&info).unwrap_or_default()).await;
            sys::db_set_text(CURRENT_KEY, &id).await;
        }
        sys::db_delete(LEGACY_KEY).await;
    }
    let mut entries = Vec::new();
    for key in sys::db_keys().await {
        let Some(id) = key.strip_prefix(INFO_PREFIX) else { continue };
        let Some(info) = sys::db_get_text(&key).await.and_then(|t| serde_json::from_str::<Info>(&t).ok()) else { continue };
        let thumb_png = sys::db_get_bytes(&thumb_key(id)).await;
        entries.push(Entry { id: id.to_string(), info, thumb_png, thumb: None });
    }
    entries.sort_by(|a, b| b.info.modified.total_cmp(&a.info.modified));
    (entries, sys::db_get_text(CURRENT_KEY).await)
}

/// Lee un proyecto y deja sus fotos y su fuente listas en memoria.
async fn load_project(id: &str) -> Option<Saved> {
    let saved: Saved = serde_json::from_str(&sys::db_get_text(&doc_key(id)).await?).ok()?;
    for p in project::doc_files(&saved.doc) {
        if !sys::has_file(&p) {
            if let Some(bytes) = sys::db_get_bytes(&file_key(&p)).await {
                sys::store_at(p, bytes);
            }
        }
    }
    Some(saved)
}

/// Borra las fotos que ya no usa ningún proyecto.
async fn collect_garbage(used: HashSet<String>) {
    for key in sys::db_keys().await {
        if key.strip_prefix(FILE_PREFIX).is_some_and(|p| !used.contains(p)) {
            sys::db_delete(&key).await;
        }
    }
}

fn used_files(entries: &[Entry]) -> HashSet<String> {
    entries.iter().flat_map(|e| e.info.files.iter().cloned()).collect()
}

/// Lo que se pide desde la ventana de proyectos.
enum ProjectAction {
    Open(String),
    Duplicate(String),
    Delete(String),
    Rename(String, String),
    New,
    Import,
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
            let (entries, current) = load_index().await;
            let current = current.filter(|c| entries.iter().any(|e| &e.id == c)).or_else(|| entries.first().map(|e| e.id.clone()));
            let saved = match &current {
                Some(id) => load_project(id).await.map(|s| (id.clone(), s)),
                None => None,
            };
            collect_garbage(used_files(&entries)).await;
            Self::send_job(&tx, &ctx, move |app, ctx| app.finish_restore(entries, saved, ctx));
        });
    }

    fn finish_restore(&mut self, entries: Vec<Entry>, saved: Option<(String, Saved)>, ctx: &egui::Context) {
        self.web.projects = entries;
        self.web.restored = true;
        // Si empezaste a trabajar mientras tanto, eso sigue como proyecto nuevo.
        let untouched = self.doc.layers.is_empty() && self.undo.is_empty();
        match saved.filter(|_| untouched) {
            Some((id, s)) => {
                let has_content = !s.doc.layers.is_empty();
                self.load_into(id, s, ctx);
                if has_content {
                    self.status = format!("Se abrió «{}», tu último proyecto.", self.doc_name());
                }
            }
            None => {
                self.web.current = new_id();
                self.web.saved_json.clear();
            }
        }
        // Para entonces los paneles ya tienen su tamaño final.
        self.fit_request = true;
    }

    /// Pone en pantalla un proyecto leído del navegador.
    fn load_into(&mut self, id: String, s: Saved, ctx: &egui::Context) {
        let name = self.web.projects.iter().find(|e| e.id == id).map(|e| e.info.name.clone()).unwrap_or_else(|| "Sin título".into());
        let files = project::doc_files(&s.doc);
        self.web.saved_files = files.iter().filter(|p| sys::has_file(p)).cloned().collect();
        self.images.retain(|p, _| files.contains(p));
        self.set_doc(s.doc, Some(PathBuf::from(format!("{name}.carrusel"))), ctx);
        self.undo.clear();
        self.redo.clear();
        self.dirty = s.dirty;
        self.web.current = id;
        self.web.saved_json = self.autosave_json();
        self.web.thumb_for = hash(&self.web.saved_json);
        spawn_write(sys::db_set_text(CURRENT_KEY, &self.web.current));
    }

    fn autosave_json(&self) -> String {
        serde_json::to_string(&SavedRef { dirty: self.dirty, doc: &self.doc }).unwrap_or_default()
    }

    /// Guarda el proyecto en el navegador si cambió (como mucho una vez por segundo).
    fn autosave(&mut self) {
        let json = self.autosave_json();
        if json == self.web.saved_json {
            return;
        }
        let id = self.web.current.clone();
        let listed = self.web.projects.iter().any(|e| e.id == id);
        // Un proyecto nuevo y vacío no se guarda (no llena la lista de «Sin título»).
        if !listed && self.doc.layers.is_empty() {
            return;
        }
        // Primero las fotos nuevas, luego el documento que las usa (IndexedDB
        // respeta el orden de las escrituras).
        for p in project::doc_files(&self.doc) {
            if self.web.saved_files.contains(&p) {
                continue;
            }
            if let Some(bytes) = sys::file_bytes(&p) {
                spawn_write(sys::db_set_bytes(&file_key(&p), &bytes));
                self.web.saved_files.insert(p);
            }
        }
        spawn_write(sys::db_set_text(&doc_key(&id), &json));
        let info = Info { name: self.doc_name(), modified: sys::now_ms(), files: file_list(&self.doc) };
        spawn_write(sys::db_set_text(&info_key(&id), &serde_json::to_string(&info).unwrap_or_default()));
        match self.web.projects.iter().position(|e| e.id == id) {
            Some(i) => {
                let mut e = self.web.projects.remove(i);
                e.info = info;
                self.web.projects.insert(0, e);
            }
            None => self.web.projects.insert(0, Entry { id, info, thumb_png: None, thumb: None }),
        }
        self.web.saved_json = json;
    }

    /// Miniatura de los primeros slides del proyecto abierto, si cambió.
    fn update_thumb(&mut self) {
        let h = hash(&self.web.saved_json);
        if h == self.web.thumb_for || self.pending > 0 || self.doc.layers.is_empty() {
            return;
        }
        let Some(i) = self.web.projects.iter().position(|e| e.id == self.web.current) else { return };
        let images: HashMap<PathBuf, Arc<RgbaImage>> = self.images.iter().map(|(k, v)| (k.clone(), v.full.clone())).collect();
        let (sw, sh) = self.doc.format.size();
        let n = self.doc.slides.min(3);
        let strip = render::render_region(&self.doc, &images, &self.font, 0.0, sw * n);
        let th = 120;
        let tw = (sw * n * th / sh).max(1);
        let small = image::imageops::thumbnail(&strip, tw, th);
        if let Ok(png) = render::encode_image(small, ExportFormat::Png) {
            spawn_write(sys::db_set_bytes(&thumb_key(&self.web.current), &png));
            let e = &mut self.web.projects[i];
            e.thumb_png = Some(png);
            e.thumb = None;
        }
        self.web.thumb_for = h;
    }

    /// Deja guardado el proyecto abierto antes de cambiar a otro.
    fn flush(&mut self) {
        self.autosave();
        self.update_thumb();
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
    // Ventana «Proyectos»

    /// «Proyectos…» (y ⌘O): la lista de proyectos guardados en el navegador.
    pub(crate) fn web_open(&mut self, _ctx: &egui::Context) {
        if self.web.restored {
            self.flush();
        }
        self.web.show_projects = true;
    }

    pub(crate) fn web_new_project(&mut self, ctx: &egui::Context) {
        if self.web.restored {
            self.flush();
        }
        self.start_new_project(ctx);
        self.status = "Proyecto nuevo. El anterior sigue en «Proyectos…».".into();
    }

    fn start_new_project(&mut self, ctx: &egui::Context) {
        self.set_doc(Doc::default(), None, ctx);
        self.undo.clear();
        self.redo.clear();
        self.web.current = new_id();
        self.web.saved_json.clear();
        self.web.saved_files.clear();
        spawn_write(sys::db_set_text(CURRENT_KEY, &self.web.current));
    }

    fn open_project(&mut self, id: String, ctx: &egui::Context) {
        self.flush();
        let (tx, ctx) = (self.web.tx.clone(), ctx.clone());
        self.status = "Abriendo…".into();
        sys::spawn(async move {
            let saved = load_project(&id).await;
            Self::send_job(&tx, &ctx, move |app, ctx| match saved {
                Some(s) => {
                    app.load_into(id, s, ctx);
                    app.status = format!("Abierto «{}».", app.doc_name());
                }
                None => app.status = "No se pudo abrir ese proyecto.".into(),
            });
        });
    }

    fn duplicate_project(&mut self, id: String, ctx: &egui::Context) {
        if id == self.web.current {
            self.flush();
        }
        let Some(e) = self.web.projects.iter().find(|e| e.id == id) else { return };
        let mut info = e.info.clone();
        info.name = format!("{} (copia)", info.name);
        info.modified = sys::now_ms();
        let thumb = e.thumb_png.clone();
        let (tx, ctx) = (self.web.tx.clone(), ctx.clone());
        sys::spawn(async move {
            let Some(text) = sys::db_get_text(&doc_key(&id)).await else { return };
            let new = new_id();
            sys::db_set_text(&doc_key(&new), &text).await;
            sys::db_set_text(&info_key(&new), &serde_json::to_string(&info).unwrap_or_default()).await;
            if let Some(png) = &thumb {
                sys::db_set_bytes(&thumb_key(&new), png).await;
            }
            Self::send_job(&tx, &ctx, move |app, _| {
                app.status = format!("Creado «{}».", info.name);
                app.web.projects.insert(0, Entry { id: new, info, thumb_png: thumb, thumb: None });
            });
        });
    }

    fn delete_project(&mut self, id: String, ctx: &egui::Context) {
        let Some(i) = self.web.projects.iter().position(|e| e.id == id) else { return };
        let name = self.web.projects[i].info.name.clone();
        if !sys::confirm(&format!("¿Eliminar «{name}» de este navegador?\n\nNo se puede deshacer.")) {
            return;
        }
        self.web.projects.remove(i);
        if id == self.web.current {
            // No se guarda nada más de él: se abre otro (o uno nuevo).
            self.web.saved_json = self.autosave_json();
            match self.web.projects.first().map(|e| e.id.clone()) {
                Some(next) => self.open_project_without_flush(next, ctx),
                None => self.start_new_project(ctx),
            }
        }
        let used = used_files(&self.web.projects);
        sys::spawn(async move {
            for key in [doc_key(&id), info_key(&id), thumb_key(&id)] {
                sys::db_delete(&key).await;
            }
            collect_garbage(used).await;
        });
        self.status = format!("Eliminado «{name}».");
    }

    fn open_project_without_flush(&mut self, id: String, ctx: &egui::Context) {
        let (tx, ctx) = (self.web.tx.clone(), ctx.clone());
        sys::spawn(async move {
            let saved = load_project(&id).await;
            Self::send_job(&tx, &ctx, move |app, ctx| match saved {
                Some(s) => app.load_into(id, s, ctx),
                None => app.start_new_project(ctx),
            });
        });
    }

    fn rename_project(&mut self, id: String, name: String) {
        let name = clean_name(&name);
        if id == self.web.current {
            self.path = Some(PathBuf::from(format!("{name}.carrusel")));
        }
        if let Some(e) = self.web.projects.iter_mut().find(|e| e.id == id) {
            e.info.name = name;
            spawn_write(sys::db_set_text(&info_key(&id), &serde_json::to_string(&e.info).unwrap_or_default()));
        }
    }

    /// Importa un .carrusel (de la Mac o descargado antes) como proyecto nuevo.
    fn import_project(&mut self, ctx: &egui::Context) {
        let picked = sys::pick_files("", false);
        let (tx, ctx) = (self.web.tx.clone(), ctx.clone());
        sys::spawn(async move {
            let Some(f) = picked.await.into_iter().next() else { return };
            Self::send_job(&tx, &ctx, move |app, ctx| app.import_file(&f.name, &f.bytes, ctx));
        });
    }

    fn import_file(&mut self, file_name: &str, bytes: &[u8], ctx: &egui::Context) {
        let opened = match project::parse(bytes) {
            Ok(o) => o,
            Err(e) => {
                self.status = format!("No se pudo abrir «{file_name}»: {e}");
                return;
            }
        };
        for (path, data) in opened.files {
            sys::store_at(path, data);
        }
        let missing = project::doc_files(&opened.doc).iter().filter(|p| !sys::has_file(p)).count();
        let name = clean_name(&Path::new(file_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
        self.flush();
        self.start_new_project(ctx);
        self.set_doc(opened.doc, Some(PathBuf::from(format!("{name}.carrusel"))), ctx);
        self.undo.clear();
        self.web.show_projects = false;
        self.status = if missing > 0 {
            format!("Abierto «{name}», pero faltan {missing} archivo(s): en la Mac guárdalo con «Guardar con fotos…».")
        } else {
            format!("Abierto «{name}».")
        };
    }

    /// Ventana con los proyectos guardados en este navegador.
    pub(crate) fn projects_window(&mut self, ctx: &egui::Context) {
        if !self.web.show_projects {
            return;
        }
        let mut open = true;
        let mut action = None;
        egui::Window::new("Proyectos")
            .open(&mut open)
            .default_size([520.0, 520.0])
            .collapsible(false)
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("＋ Proyecto nuevo").clicked() {
                        action = Some(ProjectAction::New);
                    }
                    if ui.button("Importar .carrusel…").on_hover_text("Un proyecto guardado en la Mac o descargado antes").clicked() {
                        action = Some(ProjectAction::Import);
                    }
                });
                ui.weak("Se guardan solos en este navegador. Para tener una copia o llevar uno a la Mac, ábrelo y toca «Guardar».");
                ui.separator();
                if self.web.projects.is_empty() {
                    ui.weak("Todavía no hay proyectos guardados.");
                    return;
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for i in 0..self.web.projects.len() {
                        if let Some(a) = self.project_row(ui, i) {
                            action = Some(a);
                        }
                        ui.separator();
                    }
                });
            });
        if !open {
            self.web.show_projects = false;
            self.web.renaming = None;
        }
        match action {
            Some(ProjectAction::Open(id)) => {
                self.web.show_projects = false;
                self.open_project(id, ctx);
            }
            Some(ProjectAction::Duplicate(id)) => self.duplicate_project(id, ctx),
            Some(ProjectAction::Delete(id)) => self.delete_project(id, ctx),
            Some(ProjectAction::Rename(id, name)) => self.rename_project(id, name),
            Some(ProjectAction::New) => {
                self.web.show_projects = false;
                self.web_new_project(ctx);
            }
            Some(ProjectAction::Import) => self.import_project(ctx),
            None => {}
        }
    }

    fn project_row(&mut self, ui: &mut egui::Ui, i: usize) -> Option<ProjectAction> {
        let ctx = ui.ctx().clone();
        let current = self.web.projects[i].id == self.web.current;
        let mut action = None;
        ui.horizontal(|ui| {
            let e = &mut self.web.projects[i];
            if e.thumb.is_none() {
                if let Some(img) = e.thumb_png.as_ref().and_then(|b| image::load_from_memory(b).ok()).map(|i| i.to_rgba8()) {
                    let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                    e.thumb = Some(ctx.load_texture(format!("proyecto:{}", e.id), color, egui::TextureOptions::LINEAR));
                }
            }
            let box_size = Vec2::new(150.0, 64.0);
            let (rect, _) = ui.allocate_exact_size(box_size, egui::Sense::hover());
            ui.painter().rect_filled(rect, 4.0, Color32::from_gray(45));
            if let Some(t) = &e.thumb {
                let s = (box_size.x / t.size_vec2().x).min(box_size.y / t.size_vec2().y);
                let r = egui::Rect::from_center_size(rect.center(), t.size_vec2() * s);
                let full = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0));
                ui.painter().image(t.id(), r, full, Color32::WHITE);
            }
            let id = e.id.clone();
            let mut cancel = false;
            ui.vertical(|ui| {
                match &mut self.web.renaming {
                    Some((rid, text)) if *rid == id => {
                        let resp = ui.add(egui::TextEdit::singleline(text).desired_width(220.0));
                        resp.request_focus();
                        let enter = ui.input(|inp| inp.key_pressed(egui::Key::Enter));
                        ui.horizontal(|ui| {
                            if ui.button("Aceptar").clicked() || enter {
                                action = Some(ProjectAction::Rename(id.clone(), text.clone()));
                            }
                            if ui.button("Cancelar").clicked() {
                                cancel = true;
                            }
                        });
                    }
                    _ => {
                        let e = &self.web.projects[i];
                        let mut title = egui::RichText::new(&e.info.name).strong();
                        if current {
                            title = title.color(ACCENT);
                        }
                        ui.label(title);
                        let when = format_date(e.info.modified);
                        ui.weak(if current { format!("Abierto · {when}") } else { when });
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!current, egui::Button::new("Abrir")).clicked() {
                                action = Some(ProjectAction::Open(id.clone()));
                            }
                            if ui.button("Duplicar").clicked() {
                                action = Some(ProjectAction::Duplicate(id.clone()));
                            }
                            if ui.button("Renombrar").clicked() {
                                self.web.renaming = Some((id.clone(), e.info.name.clone()));
                            }
                            if ui.button(egui::RichText::new("Eliminar").color(Color32::from_rgb(255, 120, 120))).clicked() {
                                action = Some(ProjectAction::Delete(id.clone()));
                            }
                        });
                    }
                }
            });
            if cancel {
                self.web.renaming = None;
            }
        });
        if matches!(action, Some(ProjectAction::Rename(..))) {
            self.web.renaming = None;
        }
        action
    }

    // -----------------------------------------------------------------------
    // Fotos y fuentes

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

    /// «Guardar» descarga el proyecto con sus fotos en un solo archivo, que
    /// también abre la app de la Mac.
    pub(crate) fn web_save(&mut self) {
        let (json, _) = match project::bundle(&self.doc, |p| sys::file_bytes(p).map(|b| b.as_ref().clone())) {
            Ok(r) => r,
            Err(e) => {
                self.status = format!("No se pudo guardar: {e}");
                return;
            }
        };
        let name = format!("{}.carrusel", self.doc_name());
        sys::download(&name, &json, "application/octet-stream");
        self.dirty = false;
        self.status = format!("Proyecto descargado: «{name}» (en iPad queda en Archivos › Descargas; ábrelo en la Mac con «Abrir…»).");
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
                    if ui.button("Descargar uno por uno").on_hover_text("Cada slide como su propio archivo (el navegador puede pedir permiso)").clicked() {
                        for (name, bytes, mime) in &export.files {
                            sys::download(name, bytes, mime);
                        }
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
