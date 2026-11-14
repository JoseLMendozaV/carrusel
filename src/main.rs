//! Carrusel — creador de carruseles panorámicos para Instagram (estilo SCRL).
//! Corre como app de escritorio y, compilado a WebAssembly, en el navegador
//! (pensado para iPad): ver `sys` y `web_app`.

// En la web no se usan las partes de escritorio (videos, exportar a carpeta…).
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

mod doc;
mod layers;
mod library;
mod online;
mod render;
mod scrapbook;
mod sys;
mod updater;
mod vector;
mod video;
#[cfg(target_arch = "wasm32")]
mod web_app;

use ab_glyph::FontArc;
use doc::{fit, Crop, Doc, Edge, Format, FrameKind, Kind, Layer, ShapeKind, TextAlign, INSTAGRAM_MAX, LINE_H};
use eframe::egui::{
    self, epaint::RectShape, Align2, Color32, CornerRadius, FontFamily, FontId, Key, KeyboardShortcut, Modifiers,
    Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextureHandle, Vec2,
};
use image::RgbaImage;
use render::ExportFormat;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};

const IMAGE_EXT: &[&str] = &["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff"];
/// En la web el navegador decodifica también estos (Safari entiende HEIC).
const WEB_IMAGE_EXT: &[&str] = &["heic", "heif", "avif"];
/// Texto que añade el botón «＋ Texto».
const DEFAULT_TEXT: library::TextPreset = library::TextPreset {
    label: "Texto",
    text: "Tu texto",
    size: 96.0,
    color: [20, 20, 20, 255],
    align: TextAlign::Center,
    background: None,
    shadow: false,
};

fn is_supported(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).unwrap_or_default();
    video::is_video(path) || IMAGE_EXT.contains(&ext.as_str()) || (sys::WEB && WEB_IMAGE_EXT.contains(&ext.as_str()))
}
const ACCENT: Color32 = Color32::from_rgb(255, 64, 129);
const CANVAS_FONT: &str = "canvas";
/// Límites del zoom; el mínimo es muy pequeño para poder ver carruseles larguísimos.
const ZOOM_MIN: f32 = 0.002;
const ZOOM_MAX: f32 = 2.0;

enum ExportMsg {
    Progress(f32),
    Done(Result<String, String>),
}

/// Estado de las actualizaciones (ver `updater`).
enum Update {
    Idle,
    Checking,
    UpToDate,
    Available(updater::Release),
    Downloading { release: updater::Release, progress: f32 },
    /// Descargada y verificada: se instala al reiniciar (o al cerrar la app).
    Ready { release: updater::Release, app: PathBuf },
    Failed(String),
}

enum UpdateMsg {
    Checked { manual: bool, result: Result<updater::Release, String> },
    Progress(f32),
    Downloaded(Result<PathBuf, String>),
}

/// Qué se exporta.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExportTarget {
    /// Un archivo por slide: foto (JPG/PNG) o video MP4 si el slide tiene video.
    Carousel,
    /// Un único video vertical 9:16 que recorre el carrusel (Reels / TikTok).
    ScrollVideo,
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Carrusel")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([800.0, 500.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native("Carrusel", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

#[cfg(target_arch = "wasm32")]
fn main() {
    web_app::start();
}

/// Controles algo más grandes y separados para usarlos con el dedo.
fn touch_style(ctx: &egui::Context) {
    ctx.all_styles_mut(|s| {
        s.spacing.interact_size.y = 30.0;
        s.spacing.button_padding = Vec2::new(10.0, 6.0);
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.slider_width = 130.0;
        s.spacing.icon_width = 18.0;
        s.spacing.combo_height = 320.0;
        for font in s.text_styles.values_mut() {
            font.size += 1.0;
        }
    });
}

// ---------------------------------------------------------------------------

struct Loaded {
    full: Arc<RgbaImage>,
    tex: TextureHandle,
    aspect: f32,
}

/// Qué hacer cuando termina de cargarse una imagen.
#[derive(Clone, Copy)]
enum Place {
    /// Solo cachearla (al abrir un proyecto).
    Cache,
    /// Crear una capa que cubra este slide.
    Slide(u32),
    /// Crear una capa centrada en este punto del lienzo.
    At(f32, f32),
}

enum LoadedData {
    Image(RgbaImage, egui::ColorImage),
    /// Información del video y su primer fotograma.
    Video(video::VideoInfo, egui::ColorImage),
}

struct LoadMsg {
    path: PathBuf,
    place: Place,
    result: Result<LoadedData, String>,
}

struct VideoEntry {
    info: video::VideoInfo,
    poster: TextureHandle,
}

/// Textura de una capa vectorial, con la clave de lo que contiene.
struct VectorTex {
    key: u64,
    tex: TextureHandle,
}

#[derive(Clone, Copy, PartialEq)]
enum DragMode {
    Move,
    Resize,
    /// Recortar arrastrando un lado de la foto.
    Crop(Edge),
    /// Mover la foto dentro de su recorte (⌘ + arrastrar).
    Pan,
    /// Estirar un lado de una forma o un marco.
    Stretch(Edge),
}

struct Drag {
    mode: DragMode,
    id: u64,
    start: Layer,
    start_pointer: Pos2, // en coordenadas del lienzo
}

/// Vista previa tipo Instagram: un slide a la vez.
struct Preview {
    index: u32,
    /// Posición animada (en slides) que persigue a `index`.
    pos: f32,
    /// Arrastre horizontal en curso, en píxeles de pantalla.
    drag_dx: f32,
}

struct App {
    doc: Doc,
    path: Option<PathBuf>,
    dirty: bool,
    undo: Vec<Doc>,
    redo: Vec<Doc>,
    /// Mientras se arrastra un control o se escribe, los cambios se agrupan
    /// en un único paso de deshacer.
    coalesce: bool,
    allow_close: bool,
    preview: Option<Preview>,
    menu_slide: u32,

    images: HashMap<PathBuf, Loaded>,
    videos: HashMap<PathBuf, VideoEntry>,
    /// Reproductores de video del editor y su último fotograma, por capa.
    players: HashMap<u64, video::Player>,
    video_tex: HashMap<u64, TextureHandle>,
    play_videos: bool,
    vector_tex: HashMap<u64, VectorTex>,
    library: library::Library,
    layers_ui: layers::LayersUi,
    /// Slide donde se colocan los elementos nuevos (el último en el que se hizo clic).
    focus_slide: u32,
    failed: HashMap<PathBuf, String>,
    pending: usize,
    load_tx: mpsc::Sender<LoadMsg>,
    load_rx: mpsc::Receiver<LoadMsg>,

    font: FontArc,
    font_loaded_for: Option<Option<PathBuf>>,

    selected: Option<u64>,
    drag: Option<Drag>,
    guides: Vec<(bool, f32)>, // (vertical, coordenada en lienzo)
    zoom: f32,
    fit_request: bool,
    snap: bool,
    span: u32,

    export_format: ExportFormat,
    export_panorama: bool,
    export_target: ExportTarget,
    scroll_seconds: f32,
    update: Update,
    update_tx: mpsc::Sender<UpdateMsg>,
    update_rx: mpsc::Receiver<UpdateMsg>,
    show_update_notes: bool,
    exporting: Option<mpsc::Receiver<ExportMsg>>,
    status: String,
    /// Arrastrar una foto la mueve dentro de su recorte (lo que en la Mac
    /// se hace con ⌘ + arrastrar), para usarlo con el dedo.
    pan_crop_mode: bool,
    /// Se está desplazando el lienzo arrastrando el fondo.
    panning: bool,
    /// Origen del lienzo en pantalla en el fotograma anterior (para hacer
    /// zoom alrededor del puntero o de los dedos).
    last_origin: Option<Pos2>,
    #[cfg(target_arch = "wasm32")]
    web: web_app::WebState,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.selection.bg_fill = ACCENT;
        visuals.hyperlink_color = ACCENT;
        cc.egui_ctx.set_visuals(visuals);
        if sys::WEB {
            touch_style(&cc.egui_ctx);
        }

        let (load_tx, load_rx) = mpsc::channel();
        let (update_tx, update_rx) = mpsc::channel();
        let mut app = Self {
            doc: Doc::default(),
            path: None,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            coalesce: false,
            allow_close: false,
            preview: None,
            menu_slide: 0,
            images: HashMap::new(),
            videos: HashMap::new(),
            players: HashMap::new(),
            video_tex: HashMap::new(),
            play_videos: true,
            vector_tex: HashMap::new(),
            library: library::Library::default(),
            layers_ui: layers::LayersUi::default(),
            focus_slide: 0,
            failed: HashMap::new(),
            pending: 0,
            load_tx,
            load_rx,
            font: FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("fuente integrada"),
            font_loaded_for: None,
            selected: None,
            drag: None,
            guides: Vec::new(),
            zoom: 0.3,
            fit_request: true,
            snap: true,
            span: 1,
            export_format: ExportFormat::Jpg,
            export_panorama: false,
            export_target: ExportTarget::Carousel,
            scroll_seconds: 2.5,
            update: Update::Idle,
            update_tx,
            update_rx,
            show_update_notes: false,
            exporting: None,
            status: if sys::WEB {
                "Toca «＋ Fotos» para empezar.".into()
            } else {
                "Arrastra fotos o videos a la ventana, o usa «＋ Fotos / videos».".into()
            },
            pan_crop_mode: false,
            panning: false,
            last_origin: None,
            #[cfg(target_arch = "wasm32")]
            web: web_app::WebState::default(),
        };
        app.sync_font(&cc.egui_ctx);
        // Una vez al día, en segundo plano, mira si hay una versión nueva
        // (la versión web siempre es la última).
        if !sys::WEB && updater::due_for_check() {
            app.check_updates(&cc.egui_ctx, false);
        }
        #[cfg(target_arch = "wasm32")]
        app.web_init(&cc.egui_ctx);
        app
    }

    // ----- actualizaciones -------------------------------------------------

    fn check_updates(&mut self, ctx: &egui::Context, manual: bool) {
        if matches!(self.update, Update::Checking | Update::Downloading { .. } | Update::Ready { .. }) {
            return;
        }
        self.update = Update::Checking;
        let (tx, ctx) = (self.update_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = updater::latest();
            if result.is_ok() {
                updater::mark_checked();
            }
            let _ = tx.send(UpdateMsg::Checked { manual, result });
            ctx.request_repaint();
        });
    }

    fn start_update_download(&mut self, ctx: &egui::Context, release: updater::Release) {
        // Sin paquete .app (p. ej. con `cargo run`) o sin permiso para
        // reemplazarlo, se descarga a mano desde GitHub.
        match updater::current_bundle() {
            Some(bundle) if updater::can_replace(&bundle) => {}
            _ => {
                ctx.open_url(egui::OpenUrl::new_tab(&release.page));
                self.status = "Descarga la versión nueva desde la página de GitHub que se acaba de abrir.".into();
                self.update = Update::Idle;
                return;
            }
        }
        self.update = Update::Downloading { release: release.clone(), progress: 0.0 };
        let (tx, ctx) = (self.update_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let progress_ctx = ctx.clone();
            let result = updater::download(&release, &move |f| {
                let _ = progress_tx.send(UpdateMsg::Progress(f));
                progress_ctx.request_repaint();
            });
            let _ = tx.send(UpdateMsg::Downloaded(result));
            ctx.request_repaint();
        });
    }

    fn poll_updates(&mut self) {
        while let Ok(msg) = self.update_rx.try_recv() {
            self.update = match (msg, std::mem::replace(&mut self.update, Update::Idle)) {
                (UpdateMsg::Checked { manual, result: Ok(r) }, _) => {
                    if updater::is_newer(&r.version, updater::current_version()) {
                        Update::Available(r)
                    } else if manual {
                        Update::UpToDate
                    } else {
                        Update::Idle
                    }
                }
                (UpdateMsg::Checked { manual: true, result: Err(e) }, _) => Update::Failed(e),
                (UpdateMsg::Checked { manual: false, result: Err(_) }, _) => Update::Idle,
                (UpdateMsg::Progress(p), Update::Downloading { release, .. }) => Update::Downloading { release, progress: p },
                (UpdateMsg::Downloaded(Ok(app)), Update::Downloading { release, .. }) => Update::Ready { release, app },
                (UpdateMsg::Downloaded(Err(e)), _) => Update::Failed(e),
                (_, state) => state,
            };
        }
    }

    /// Prepara el cambio de versión para cuando la app se cierre.
    fn schedule_install(&mut self, relaunch: bool) -> bool {
        let Update::Ready { app, .. } = &self.update else { return false };
        let Some(bundle) = updater::current_bundle() else { return false };
        match updater::install_on_exit(app, &bundle, std::process::id(), relaunch) {
            Ok(()) => true,
            Err(e) => {
                self.update = Update::Failed(e);
                false
            }
        }
    }

    fn restart_to_update(&mut self, ctx: &egui::Context) {
        if !self.confirm_discard() {
            return;
        }
        if self.schedule_install(true) {
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Barra superior que avisa de una versión nueva.
    fn update_banner(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let current = updater::current_version();
        ui.horizontal_wrapped(|ui| match &self.update {
            Update::Available(r) => {
                let r = r.clone();
                ui.label(egui::RichText::new(format!("Hay una versión nueva de Carrusel: {} (tienes {current}).", r.version)).strong());
                if ui.add(egui::Button::new(egui::RichText::new("Actualizar").color(Color32::WHITE)).fill(ACCENT)).clicked() {
                    self.start_update_download(&ctx, r);
                }
                ui.toggle_value(&mut self.show_update_notes, "Novedades");
                if ui.button("Ahora no").clicked() {
                    self.update = Update::Idle;
                }
            }
            Update::Downloading { release, progress } => {
                ui.label(format!("Descargando Carrusel {}…", release.version));
                ui.add(egui::ProgressBar::new(*progress).desired_width(220.0).show_percentage());
            }
            Update::Ready { release, .. } => {
                let version = release.version.clone();
                ui.label(egui::RichText::new(format!("Carrusel {version} está descargada y verificada.")).strong());
                if ui.add(egui::Button::new(egui::RichText::new("Reiniciar ahora").color(Color32::WHITE)).fill(ACCENT)).clicked() {
                    self.restart_to_update(&ctx);
                }
                ui.weak("o se instalará sola al cerrar la app.");
            }
            Update::Failed(e) => {
                ui.colored_label(Color32::from_rgb(255, 150, 150), format!("No se pudo actualizar: {e}"));
                if ui.button("Abrir GitHub").clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(updater::RELEASES_PAGE));
                }
                if ui.button("Cerrar").clicked() {
                    self.update = Update::Idle;
                }
            }
            _ => {}
        });
        if let (Update::Available(r), true) = (&self.update, self.show_update_notes) {
            let notes = r.notes.split("\n---").next().unwrap_or("").trim().to_string();
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                ui.label(notes);
            });
        }
    }

    // ----- historial -------------------------------------------------------

    fn checkpoint(&mut self) {
        self.push_undo(self.doc.clone());
        self.coalesce = false;
    }

    fn push_undo(&mut self, state: Doc) {
        self.undo.push(state);
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    /// Registra un cambio hecho con un control (deslizador, texto, color…).
    /// `before` es el documento antes del cambio; mientras dure el gesto solo
    /// se guarda el primero, así un arrastre es un único paso de deshacer.
    fn record(&mut self, before: Doc) {
        if !self.coalesce {
            self.push_undo(before);
        }
        self.coalesce = true;
        self.dirty = true;
    }

    fn doc_name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Sin título".into())
    }

    /// Pregunta qué hacer con los cambios sin guardar. Devuelve `true` si se
    /// puede seguir (guardado o descartado) y `false` si el usuario cancela.
    fn confirm_discard(&mut self) -> bool {
        if !self.dirty || (self.path.is_none() && self.doc.layers.is_empty()) {
            return true;
        }
        #[cfg(target_arch = "wasm32")]
        {
            sys::confirm(&format!(
                "«{}» tiene cambios que no guardaste en un archivo.\n\n¿Descartarlos? (Cancela y toca «Guardar» si quieres conservar una copia.)",
                self.doc_name()
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.confirm_discard_native()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn confirm_discard_native(&mut self) -> bool {
        let (save, discard) = ("Guardar", "No guardar");
        let answer = rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Warning)
            .set_title("Cambios sin guardar")
            .set_description(format!(
                "¿Quieres guardar los cambios de «{}»?\nSi no los guardas, se perderán.",
                self.doc_name()
            ))
            .set_buttons(rfd::MessageButtons::YesNoCancelCustom(save.into(), discard.into(), "Cancelar".into()))
            .show();
        match answer {
            rfd::MessageDialogResult::Custom(s) if s == save => {
                self.save(false);
                !self.dirty
            }
            rfd::MessageDialogResult::Yes => {
                self.save(false);
                !self.dirty
            }
            rfd::MessageDialogResult::Custom(s) if s == discard => true,
            rfd::MessageDialogResult::No => true,
            _ => false,
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.doc, prev));
            self.after_history();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.doc, next));
            self.after_history();
        }
    }

    fn after_history(&mut self) {
        self.dirty = true;
        if let Some(id) = self.selected {
            if self.doc.index_of(id).is_none() {
                self.selected = None;
            }
        }
    }

    // ----- fuentes ---------------------------------------------------------

    /// Registra en egui la fuente del documento para que la vista previa
    /// y la exportación usen exactamente la misma tipografía.
    fn sync_font(&mut self, ctx: &egui::Context) {
        if self.font_loaded_for.as_ref() == Some(&self.doc.font) {
            return;
        }
        let bytes: Option<Vec<u8>> = self.doc.font.as_ref().and_then(|p| sys::read(p).ok());
        let parsed = bytes.as_ref().and_then(|b| FontArc::try_from_vec(b.clone()).ok());
        let mut defs = egui::FontDefinitions::default();
        let data = match (bytes, parsed) {
            (Some(b), Some(f)) => {
                self.font = f;
                egui::FontData::from_owned(b)
            }
            _ => {
                if self.doc.font.is_some() {
                    self.status = "No se pudo leer la fuente; uso la integrada.".into();
                }
                self.font = FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).unwrap();
                egui::FontData::from_static(epaint_default_fonts::UBUNTU_LIGHT)
            }
        };
        defs.font_data.insert(CANVAS_FONT.into(), Arc::new(data));
        defs.families
            .insert(FontFamily::Name(CANVAS_FONT.into()), vec![CANVAS_FONT.into()]);
        ctx.set_fonts(defs);
        self.font_loaded_for = Some(self.doc.font.clone());
    }

    // ----- imágenes --------------------------------------------------------

    fn request_load(&mut self, path: PathBuf, place: Place, ctx: &egui::Context) {
        if self.images.contains_key(&path) || self.videos.contains_key(&path) {
            self.place_media(&path, place);
            return;
        }
        self.pending += 1;
        let tx = self.load_tx.clone();
        let ctx = ctx.clone();
        sys::spawn(async move {
            let result = if video::is_video(&path) {
                video::probe(&path).and_then(|info| {
                    let size = video::fit_size(&info, 640);
                    let poster = video::frame_at(&path, 0.0, size)?;
                    let color = egui::ColorImage::from_rgba_unmultiplied([size.0 as usize, size.1 as usize], poster.as_raw());
                    Ok(LoadedData::Video(info, color))
                })
            } else {
                sys::load_image(&path).await.map(|full| {
                    // Textura de vista previa (máx. 2048 px); la original se usa al exportar.
                    let s = (2048.0 / full.width().max(full.height()) as f32).min(1.0);
                    let tw = ((full.width() as f32 * s) as u32).max(1);
                    let th = ((full.height() as f32 * s) as u32).max(1);
                    let thumb = if s < 1.0 { image::imageops::thumbnail(&full, tw, th) } else { full.clone() };
                    let color = egui::ColorImage::from_rgba_unmultiplied(
                        [thumb.width() as usize, thumb.height() as usize],
                        thumb.as_raw(),
                    );
                    LoadedData::Image(full, color)
                })
            };
            let _ = tx.send(LoadMsg { path, place, result });
            ctx.request_repaint();
        });
    }

    fn receive_loads(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.load_rx.try_recv() {
            self.pending = self.pending.saturating_sub(1);
            let name = msg.path.display().to_string();
            match msg.result {
                Ok(LoadedData::Image(full, color)) => {
                    let aspect = full.width() as f32 / full.height() as f32;
                    let tex = ctx.load_texture(name, color, egui::TextureOptions::LINEAR);
                    self.images.insert(msg.path.clone(), Loaded { full: Arc::new(full), tex, aspect });
                }
                Ok(LoadedData::Video(info, color)) => {
                    let poster = ctx.load_texture(name, color, egui::TextureOptions::LINEAR);
                    self.videos.insert(msg.path.clone(), VideoEntry { info, poster });
                }
                Err(e) => {
                    self.status = format!("No se pudo abrir {e}");
                    self.failed.insert(msg.path, e);
                    continue;
                }
            }
            self.failed.remove(&msg.path);
            self.place_media(&msg.path, msg.place);
        }
    }

    /// Proporción (ancho / alto) de una foto o video ya cargado.
    fn media_aspect(&self, path: &Path) -> Option<f32> {
        self.images.get(path).map(|l| l.aspect).or_else(|| self.videos.get(path).map(|v| v.info.aspect()))
    }

    fn place_media(&mut self, path: &Path, place: Place) {
        let Some(aspect) = self.media_aspect(path) else { return };
        let (x, y, w, h) = match place {
            Place::Cache => return,
            Place::Slide(i) => {
                self.checkpoint();
                self.doc.slides = self.doc.slides.max(i + 1);
                self.doc.slides_rect(i, 1)
            }
            Place::At(cx, cy) => {
                self.checkpoint();
                let (sw, sh) = self.doc.slide_size();
                let (_, _, w, h) = fit(aspect, (0.0, 0.0, sw, sh), false);
                (cx - w / 2.0, cy - h / 2.0, w, h)
            }
        };
        let id = self.doc.new_id();
        let kind = if self.videos.contains_key(path) {
            Kind::Video { path: path.to_path_buf(), start: 0.0, muted: false }
        } else {
            Kind::Image { path: path.to_path_buf() }
        };
        let mut layer = Layer::new(id, kind, (x, y, w, h));
        if let Place::Slide(_) = place {
            // Llena el slide justo, sin invadir los de al lado.
            layer.set_cover(aspect, (x, y, w, h));
        }
        self.doc.layers.push(layer);
        self.selected = Some(id);
    }

    /// Fotos y videos nuevos: uno por slide a partir del primero libre.
    fn add_media(&mut self, paths: Vec<PathBuf>, ctx: &egui::Context) {
        let mut next = self.doc.layers.iter().filter(|l| l.is_media()).count() as u32;
        for p in paths {
            if video::is_video(&p) && video::ffmpeg().is_none() {
                self.status = video::MISSING.into();
                continue;
            }
            self.request_load(p, Place::Slide(next), ctx);
            next += 1;
        }
    }

    /// Slide donde van los elementos nuevos: el de la capa seleccionada o el
    /// último en el que se hizo clic.
    fn current_slide(&self) -> u32 {
        self.selected
            .and_then(|id| self.doc.layers.iter().find(|l| l.id == id))
            .map(|l| self.doc.slide_of(l))
            .unwrap_or(self.focus_slide)
            .min(self.doc.slides - 1)
    }

    /// Añade una capa centrada en el slide actual y la selecciona.
    fn add_centered(&mut self, kind: Kind, w: f32, h: f32) {
        self.checkpoint();
        let (sw, sh) = self.doc.slide_size();
        let slide = self.current_slide();
        let id = self.doc.new_id();
        self.doc.layers.push(Layer::new(id, kind, (slide as f32 * sw + (sw - w) / 2.0, (sh - h) / 2.0, w, h)));
        self.selected = Some(id);
    }

    fn add_text(&mut self, preset: &library::TextPreset) {
        let (w, h) = render::measure_text(&self.font, preset.text, preset.size);
        let kind = Kind::Text {
            text: preset.text.into(),
            size: preset.size,
            color: preset.color,
            align: preset.align,
            background: preset.background,
            shadow: preset.shadow,
        };
        self.add_centered(kind, w, h);
    }

    fn apply_library(&mut self, action: library::Action) {
        let (sw, _) = self.doc.slide_size();
        match action {
            library::Action::Emoji(emoji, sticker, style) => {
                let side = sw * 0.35;
                self.add_centered(Kind::Emoji { emoji: emoji.into(), sticker, style }, side, side);
            }
            library::Action::Shape(shape) => {
                let (w, h) = if shape == ShapeKind::Line { (sw * 0.6, 14.0) } else { (sw * 0.45, sw * 0.45) };
                let fill = [ACCENT.r(), ACCENT.g(), ACCENT.b(), 255];
                self.add_centered(Kind::Shape { shape, fill, stroke: [255, 255, 255, 0], stroke_w: 12.0 }, w, h);
            }
            library::Action::Frame(frame) => {
                // Sobre la foto seleccionada, o del tamaño del slide.
                let target = self
                    .selected
                    .and_then(|id| self.doc.layers.iter().find(|l| l.id == id && l.is_media()))
                    .map(|l| (l.x, l.y, l.w, l.h));
                let rect = target.unwrap_or_else(|| self.doc.slides_rect(self.current_slide(), 1));
                let thickness = (rect.2.min(rect.3) * 0.04).max(4.0);
                self.checkpoint();
                let id = self.doc.new_id();
                let kind = Kind::Frame { frame, color: [255, 255, 255, 255], thickness };
                self.doc.layers.push(Layer::new(id, kind, rect));
                self.selected = Some(id);
            }
            library::Action::Text(preset) => self.add_text(preset),
            library::Action::Graphic { svg, source, name, tint } => {
                // Ilustraciones algo más grandes que iconos y stickers; siempre con su proporción.
                let side = if source.starts_with("freesvg:") { sw * 0.6 } else { sw * 0.4 };
                let aspect = vector::svg_aspect(&svg);
                let (w, h) = if aspect >= 1.0 { (side, side / aspect) } else { (side * aspect, side) };
                self.add_centered(Kind::Graphic { svg, source, name, tint, sticker: false }, w, h);
            }
        }
    }

    /// Recalcula el tamaño de las capas de texto (cambian al editar).
    fn sync_text_bounds(&mut self) {
        for l in &mut self.doc.layers {
            if let Kind::Text { text, size, .. } = &l.kind {
                let (w, h) = render::measure_text(&self.font, text, *size);
                l.w = w;
                l.h = h;
            }
        }
    }

    fn distribute(&mut self) {
        let ids: Vec<u64> = self.doc.layers.iter().filter(|l| l.is_media()).map(|l| l.id).collect();
        if ids.is_empty() {
            return;
        }
        self.checkpoint();
        self.doc.slides = self.doc.slides.max(ids.len() as u32);
        for (i, id) in ids.into_iter().enumerate() {
            let aspect = self.aspect_of(id);
            let rect = self.doc.slides_rect(i as u32, 1);
            if let Some(l) = self.doc.layer_mut(id) {
                l.set_cover(aspect, rect);
            }
        }
    }

    fn aspect_of(&self, id: u64) -> f32 {
        self.doc
            .layers
            .iter()
            .find(|l| l.id == id)
            .and_then(|l| match &l.kind {
                Kind::Image { path } | Kind::Video { path, .. } => self.media_aspect(path),
                _ => None,
            })
            .unwrap_or_else(|| {
                let l = self.doc.layers.iter().find(|l| l.id == id);
                l.map(|l| l.w / l.h.max(1.0)).unwrap_or(1.0)
            })
    }

    fn fit_selected(&mut self, rect: (f32, f32, f32, f32), cover: bool) {
        let Some(id) = self.selected else { return };
        let aspect = self.aspect_of(id);
        self.checkpoint();
        if let Some(l) = self.doc.layer_mut(id) {
            if cover {
                // Llena el área justa recortando la foto, sin salirse a otros slides.
                l.set_cover(aspect, rect);
            } else {
                l.crop = Crop::FULL;
                (l.x, l.y, l.w, l.h) = fit(aspect, rect, false);
            }
        }
    }

    fn reset_crop_selected(&mut self) {
        let Some(id) = self.selected else { return };
        if self.doc.layers.iter().any(|l| l.id == id && !l.crop.is_full()) {
            self.checkpoint();
            if let Some(l) = self.doc.layer_mut(id) {
                l.reset_crop();
            }
        }
    }

    fn delete_selected(&mut self) {
        if let Some(i) = self.selected.and_then(|id| self.doc.index_of(id)) {
            self.checkpoint();
            self.doc.layers.remove(i);
            self.selected = None;
        }
    }

    fn duplicate_selected(&mut self) {
        if let Some(i) = self.selected.and_then(|id| self.doc.index_of(id)) {
            self.checkpoint();
            let mut l = self.doc.layers[i].clone();
            l.id = self.doc.new_id();
            l.x += 40.0;
            l.y += 40.0;
            self.selected = Some(l.id);
            self.doc.layers.insert(i + 1, l);
        }
    }

    fn insert_slide(&mut self, at: u32) {
        self.checkpoint();
        self.doc.insert_slide(at);
        self.fit_request = true;
        self.status = format!("Slide insertado en la posición {}.", at + 1);
    }

    fn remove_slide(&mut self, at: u32) {
        let before = self.doc.clone();
        if self.doc.remove_slide(at) {
            self.push_undo(before);
            self.after_history();
            self.fit_request = true;
            self.status = format!("Slide {} eliminado (⌘Z para deshacer).", at + 1);
        }
    }

    fn reorder(&mut self, to: isize, absolute: bool) {
        let Some(i) = self.selected.and_then(|id| self.doc.index_of(id)) else { return };
        let n = self.doc.layers.len() as isize;
        let j = if absolute { to } else { i as isize + to }.clamp(0, n - 1) as usize;
        if j != i {
            self.checkpoint();
            let l = self.doc.layers.remove(i);
            self.doc.layers.insert(j, l);
        }
    }

    // ----- archivos --------------------------------------------------------

    fn new_doc(&mut self, ctx: &egui::Context) {
        if !self.confirm_discard() {
            return;
        }
        self.checkpoint();
        self.doc = Doc::default();
        self.path = None;
        self.selected = None;
        self.fit_request = true;
        self.dirty = false;
        self.sync_font(ctx);
    }

    fn open(&mut self, ctx: &egui::Context) {
        if !self.confirm_discard() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        self.web_open(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.open_native(ctx);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_native(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new().add_filter("Proyecto Carrusel", &["carrusel"]).pick_file() else {
            return;
        };
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<Doc>(&s).map_err(|e| e.to_string()))
        {
            Ok(doc) => {
                self.set_doc(doc, Some(path.clone()), ctx);
                self.status = format!("Abierto {}", path.display());
            }
            Err(e) => self.status = format!("No se pudo abrir el proyecto: {e}"),
        }
    }

    /// Cambia al documento `doc` (recién abierto) y empieza a cargar sus fotos.
    fn set_doc(&mut self, doc: Doc, path: Option<PathBuf>, ctx: &egui::Context) {
        self.checkpoint();
        self.doc = doc;
        self.path = path;
        self.selected = None;
        self.fit_request = true;
        self.dirty = false;
        let paths: Vec<PathBuf> = self
            .doc
            .layers
            .iter()
            .filter_map(|l| match &l.kind {
                Kind::Image { path } | Kind::Video { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect();
        for p in paths {
            if !self.images.contains_key(&p) && !self.videos.contains_key(&p) {
                self.request_load(p, Place::Cache, ctx);
            }
        }
        self.sync_font(ctx);
    }

    fn save(&mut self, save_as: bool) {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = save_as;
            self.web_save();
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.save_native(save_as);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_native(&mut self, save_as: bool) {
        let path = match (&self.path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match rfd::FileDialog::new()
                .add_filter("Proyecto Carrusel", &["carrusel"])
                .set_file_name("mi-carrusel.carrusel")
                .save_file()
            {
                Some(p) => p,
                None => return,
            },
        };
        match serde_json::to_string_pretty(&self.doc).map_err(|e| e.to_string()).and_then(|s| {
            std::fs::write(&path, s).map_err(|e| e.to_string())
        }) {
            Ok(()) => {
                self.status = format!("Guardado en {}", path.display());
                self.path = Some(path);
                self.dirty = false;
            }
            Err(e) => self.status = format!("No se pudo guardar: {e}"),
        }
    }

    fn start_export(&mut self, ctx: &egui::Context) {
        if self.exporting.is_some() {
            return;
        }
        if self.pending > 0 {
            self.status = "Espera a que terminen de cargar las fotos y videos.".into();
            return;
        }
        let needs_ffmpeg = self.export_target == ExportTarget::ScrollVideo || self.doc.layers.iter().any(|l| l.is_video());
        if needs_ffmpeg && video::ffmpeg().is_none() {
            self.status = video::MISSING.into();
            return;
        }
        #[cfg(target_arch = "wasm32")]
        self.web_export(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.export_native(ctx);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn export_native(&mut self, ctx: &egui::Context) {
        let target = match self.export_target {
            ExportTarget::Carousel => rfd::FileDialog::new().set_title("Carpeta de exportación").pick_folder(),
            ExportTarget::ScrollVideo => rfd::FileDialog::new()
                .set_title("Guardar video 9:16")
                .add_filter("Video MP4", &["mp4"])
                .set_file_name(format!("{}-9x16.mp4", self.doc_name()))
                .save_file(),
        };
        let Some(target) = target else { return };
        let doc = self.doc.clone();
        let images: HashMap<PathBuf, Arc<RgbaImage>> =
            self.images.iter().map(|(k, v)| (k.clone(), v.full.clone())).collect();
        let font = self.font.clone();
        let (kind, fmt, pano, secs) = (self.export_target, self.export_format, self.export_panorama, self.scroll_seconds);
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let progress_tx = tx.clone();
            let progress_ctx = repaint.clone();
            let progress = move |f: f32| {
                let _ = progress_tx.send(ExportMsg::Progress(f));
                progress_ctx.request_repaint();
            };
            let result = match kind {
                ExportTarget::Carousel => render::export(&doc, &images, &font, &target, fmt, pano, &progress).map(|r| {
                    let mut msg = format!("✔ {} slides exportadas en {}", r.slides, target.display());
                    if r.videos > 0 {
                        msg += &format!(" ({} en video MP4)", r.videos);
                    }
                    if r.panorama_png {
                        msg += " · el panorama sale en PNG: es demasiado ancho para JPG";
                    }
                    msg
                }),
                ExportTarget::ScrollVideo => render::export_scroll_video(&doc, &images, &font, &target, secs, &progress)
                    .map(|_| format!("✔ Video 9:16 guardado en {}", target.display())),
            };
            let _ = tx.send(ExportMsg::Done(result));
            repaint.request_repaint();
        });
        self.exporting = Some(rx);
        self.status = "Exportando…".into();
    }

    fn poll_export(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.exporting else { return };
        loop {
            match rx.try_recv() {
                Ok(ExportMsg::Progress(f)) => self.status = format!("Exportando… {:.0} %", f * 100.0),
                Ok(ExportMsg::Done(result)) => {
                    self.status = result.unwrap_or_else(|e| format!("Error al exportar: {e}"));
                    self.exporting = None;
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.exporting = None;
                    return;
                }
            }
        }
    }

    // ----- atajos ----------------------------------------------------------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = |k| KeyboardShortcut::new(Modifiers::COMMAND, k);
        let cmd_shift = |k| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, k);
        if ctx.input_mut(|i| i.consume_shortcut(&cmd_shift(Key::Z))) {
            self.redo();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::Z))) {
            self.undo();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd_shift(Key::S))) {
            self.save(true);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::S))) {
            self.save(false);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::O))) {
            self.open(ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::E))) {
            self.start_export(ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::N))) {
            self.new_doc(ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&cmd(Key::D))) {
            self.duplicate_selected();
        }
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::P) && i.modifiers.is_none()) {
            self.toggle_preview();
            return;
        }
        if let Some(pv) = &mut self.preview {
            let (prev, next, exit) = ctx.input(|i| {
                (i.key_pressed(Key::ArrowLeft), i.key_pressed(Key::ArrowRight), i.key_pressed(Key::Escape))
            });
            if prev {
                pv.index = pv.index.saturating_sub(1);
            }
            if next {
                pv.index = (pv.index + 1).min(self.doc.slides - 1);
            }
            if exit {
                self.preview = None;
            }
            return;
        }
        let (del, nudge) = ctx.input(|i| {
            let step = if i.modifiers.shift { 10.0 } else { 1.0 };
            let mut d = Vec2::ZERO;
            if i.key_pressed(Key::ArrowLeft) {
                d.x -= step;
            }
            if i.key_pressed(Key::ArrowRight) {
                d.x += step;
            }
            if i.key_pressed(Key::ArrowUp) {
                d.y -= step;
            }
            if i.key_pressed(Key::ArrowDown) {
                d.y += step;
            }
            (i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace), d)
        });
        if del {
            self.delete_selected();
        }
        if nudge != Vec2::ZERO {
            if let Some(id) = self.selected.filter(|id| self.doc.layers.iter().any(|l| l.id == *id && !l.locked)) {
                self.checkpoint();
                if let Some(l) = self.doc.layer_mut(id) {
                    l.x += nudge.x;
                    l.y += nudge.y;
                }
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.selected = None;
        }
    }

    /// Líneas a las que se pega el imán: bordes y centros de cada slide y de
    /// las demás capas. Devuelve (verticales, horizontales).
    fn snap_targets(&self, exclude: u64) -> (Vec<f32>, Vec<f32>) {
        let (sw, sh) = self.doc.slide_size();
        let mut xs: Vec<f32> = (0..=self.doc.slides * 2).map(|k| k as f32 * sw / 2.0).collect();
        let mut ys = vec![0.0, sh / 2.0, sh];
        for o in self.doc.layers.iter().filter(|o| o.id != exclude) {
            xs.extend([o.x, o.x + o.w / 2.0, o.x + o.w]);
            ys.extend([o.y, o.y + o.h / 2.0, o.y + o.h]);
        }
        (xs, ys)
    }

    fn toggle_preview(&mut self) {
        self.preview = match self.preview {
            Some(_) => None,
            None => {
                let index = self
                    .selected
                    .and_then(|id| self.doc.layers.iter().find(|l| l.id == id))
                    .map(|l| self.doc.slide_of(l))
                    .unwrap_or(0);
                self.drag = None;
                Some(Preview { index, pos: index as f32, drag_dx: 0.0 })
            }
        };
    }

    // ----- UI --------------------------------------------------------------

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Nuevo").on_hover_text("⌘N").clicked() {
                self.new_doc(&ctx);
            }
            if ui.button("Abrir…").on_hover_text("⌘O").clicked() {
                self.open(&ctx);
            }
            let save_tip = if sys::WEB { "Descarga el proyecto (.carrusel) con sus fotos, para tener una copia" } else { "⌘S" };
            if ui.button("Guardar").on_hover_text(save_tip).clicked() {
                self.save(false);
            }
            ui.separator();
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Deshacer")).on_hover_text("⌘Z").clicked() {
                self.undo();
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Rehacer")).on_hover_text("⇧⌘Z").clicked() {
                self.redo();
            }
            ui.separator();
            #[cfg(target_arch = "wasm32")]
            if ui.button("＋ Fotos").clicked() {
                self.web_pick_media(&ctx);
            }
            #[cfg(not(target_arch = "wasm32"))]
            if ui.button("＋ Fotos / videos").clicked() {
                let all: Vec<&str> = IMAGE_EXT.iter().chain(video::VIDEO_EXT).copied().collect();
                if let Some(paths) = rfd::FileDialog::new()
                    .add_filter("Fotos y videos", &all)
                    .add_filter("Fotos", IMAGE_EXT)
                    .add_filter("Videos", video::VIDEO_EXT)
                    .pick_files()
                {
                    self.add_media(paths, &ctx);
                }
            }
            if ui.button("＋ Texto").clicked() {
                self.add_text(&DEFAULT_TEXT);
            }
            ui.toggle_value(&mut self.library.open, "Biblioteca")
                .on_hover_text("Emojis, calcomanías, formas, marcos y estilos de texto");
            if self.doc.layers.iter().any(|l| l.is_video()) {
                let label = if self.play_videos { "⏸ Videos" } else { "▶ Videos" };
                ui.toggle_value(&mut self.play_videos, label).on_hover_text("Reproducir los videos en el editor");
            }
            ui.separator();

            let before = self.doc.clone();
            let mut changed = false;
            egui::ComboBox::from_id_salt("format")
                .selected_text(self.doc.format.label())
                .show_ui(ui, |ui| {
                    for f in Format::ALL {
                        changed |= ui.selectable_value(&mut self.doc.format, f, f.label()).changed();
                    }
                });
            ui.label("Slides");
            changed |= ui.add(egui::DragValue::new(&mut self.doc.slides).range(1..=u32::MAX)).changed();
            if self.doc.slides > INSTAGRAM_MAX {
                ui.colored_label(Color32::from_rgb(255, 190, 80), "⚠").on_hover_text(format!(
                    "Instagram publica hasta {INSTAGRAM_MAX} slides por post. Puedes diseñar más y repartirlos en varias publicaciones."
                ));
            }
            // Insertar y quitar slides sin clic derecho (en iPad, sin mantener pulsado).
            let current = self.current_slide();
            ui.menu_button("Slide", |ui| {
                ui.weak(format!("Slide {} de {}", current + 1, self.doc.slides));
                if ui.button("Insertar slide antes").clicked() {
                    self.insert_slide(current);
                    ui.close();
                }
                if ui.button("Insertar slide después").clicked() {
                    self.insert_slide(current + 1);
                    ui.close();
                }
                if ui.add_enabled(self.doc.slides > 1, egui::Button::new("Eliminar este slide")).clicked() {
                    self.remove_slide(current);
                    ui.close();
                }
            })
            .response
            .on_hover_text("Insertar o eliminar el slide actual (también con clic derecho en el lienzo)");
            ui.label("Fondo");
            let mut bg = Color32::from_rgba_unmultiplied(
                self.doc.background[0],
                self.doc.background[1],
                self.doc.background[2],
                255,
            );
            if ui.color_edit_button_srgba(&mut bg).changed() {
                self.doc.background = [bg.r(), bg.g(), bg.b(), 255];
                changed = true;
            }
            if changed {
                self.record(before);
                self.fit_request = true;
            }
            ui.separator();
            // En la web el zoom se hace pellizcando (el deslizador no cabe en la barra).
            if !sys::WEB {
                ui.add_enabled(
                    self.preview.is_none(),
                    egui::Slider::new(&mut self.zoom, ZOOM_MIN..=ZOOM_MAX).text("Zoom").logarithmic(true),
                );
            }
            if ui.add_enabled(self.preview.is_none(), egui::Button::new("Ajustar")).clicked() {
                self.fit_request = true;
            }
            ui.checkbox(&mut self.snap, "Imán").on_hover_text("Mantén ⌥ al arrastrar para desactivarlo");
            ui.separator();
            let preview_label = if self.preview.is_some() { "✏ Editar" } else { "▶ Vista previa" };
            if ui
                .selectable_label(self.preview.is_some(), preview_label)
                .on_hover_text("Así se verá en Instagram, slide a slide (P)")
                .clicked()
            {
                self.toggle_preview();
            }
            ui.separator();
            if sys::WEB {
                // En la web todavía no hay videos: solo el carrusel de fotos.
                self.export_target = ExportTarget::Carousel;
            } else {
                self.export_target_combo(ui);
            }
            self.export_options(ui);
            let label = if self.exporting.is_some() { "Exportando…" } else { "Exportar" };
            let btn = egui::Button::new(egui::RichText::new(label).strong().color(Color32::WHITE)).fill(ACCENT);
            if ui.add_enabled(self.exporting.is_none(), btn).on_hover_text("⌘E").clicked() {
                self.start_export(&ctx);
            }
        });
    }

    fn export_target_combo(&mut self, ui: &mut egui::Ui) {
            egui::ComboBox::from_id_salt("exp_target")
                .selected_text(match self.export_target {
                    ExportTarget::Carousel => "Carrusel",
                    ExportTarget::ScrollVideo => "Video 9:16",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.export_target, ExportTarget::Carousel, "Carrusel (Instagram / TikTok)")
                        .on_hover_text("Un archivo por slide: foto, o MP4 si el slide tiene video");
                    ui.selectable_value(&mut self.export_target, ExportTarget::ScrollVideo, "Video 9:16 (Reels / TikTok)")
                        .on_hover_text("Un solo video vertical 1080×1920 que recorre el carrusel");
                });
    }

    fn export_options(&mut self, ui: &mut egui::Ui) {
            match self.export_target {
                ExportTarget::Carousel => {
                    egui::ComboBox::from_id_salt("exp_fmt")
                        .width(60.0)
                        .selected_text(match self.export_format {
                            ExportFormat::Jpg => "JPG",
                            ExportFormat::Png => "PNG",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.export_format, ExportFormat::Jpg, "JPG");
                            ui.selectable_value(&mut self.export_format, ExportFormat::Png, "PNG");
                        });
                    ui.checkbox(&mut self.export_panorama, "+ panorama");
                }
                ExportTarget::ScrollVideo => {
                    ui.add(egui::DragValue::new(&mut self.scroll_seconds).range(0.5..=15.0).speed(0.1).suffix(" s"))
                        .on_hover_text("Segundos que se queda cada slide antes de deslizarse al siguiente");
                    ui.weak("por slide");
                }
            }
    }

    fn inspector(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(idx) = self.selected.and_then(|id| self.doc.index_of(id)) else {
            ui.heading("Carrusel");
            ui.add_space(6.0);
            ui.label("Coloca fotos sobre el lienzo continuo: lo que cruce la línea entre slides se verá como un deslizamiento sin cortes en Instagram.");
            ui.add_space(10.0);
            if ui.button("Distribuir fotos: 1 por slide").clicked() {
                self.distribute();
            }
            ui.add_space(14.0);
            ui.label(egui::RichText::new("Tipografía").strong());
            let name = self
                .doc
                .font
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Ubuntu Light (integrada)".into());
            ui.label(name);
            ui.horizontal(|ui| {
                #[cfg(target_arch = "wasm32")]
                if ui.button("Elegir fuente…").on_hover_text("Un archivo .ttf, .otf o .ttc de Archivos").clicked() {
                    self.web_pick_font(&ctx);
                }
                #[cfg(not(target_arch = "wasm32"))]
                if ui.button("Elegir fuente…").clicked() {
                    if let Some(p) = rfd::FileDialog::new().add_filter("Fuentes", &["ttf", "otf", "ttc"]).pick_file() {
                        self.checkpoint();
                        self.doc.font = Some(p);
                        self.sync_font(&ctx);
                    }
                }
                if self.doc.font.is_some() && ui.button("Restablecer").clicked() {
                    self.checkpoint();
                    self.doc.font = None;
                    self.sync_font(&ctx);
                }
            });
            ui.add_space(14.0);
            if sys::WEB {
                ui.label(egui::RichText::new("Con el dedo").strong());
                for (k, v) in [
                    ("Arrastrar", "mover"),
                    ("Esquina rosa", "redimensionar"),
                    ("Lados blancos", "recortar foto o estirar forma"),
                    ("Pellizcar", "zoom"),
                    ("Dos dedos / fondo", "desplazar el lienzo"),
                    ("Mantener pulsado", "slides y capa"),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(k).monospace());
                        ui.weak(v);
                    });
                }
                ui.add_space(8.0);
                ui.weak("Con teclado también funcionan los atajos de la Mac (⌘Z, ⌘D, ⌫, flechas…).");
                ui.add_space(14.0);
                ui.label(egui::RichText::new(format!("Carrusel {} · web", updater::current_version())).strong());
                ui.weak("Tu proyecto se guarda solo en este navegador mientras trabajas. Toca «Guardar» para tener una copia en Archivos.");
                return;
            }
            ui.label(egui::RichText::new("Atajos").strong());
            for (k, v) in [
                ("Arrastrar", "mover"),
                ("Esquina rosa", "redimensionar (⇧ proporción)"),
                ("Lados blancos", "recortar foto/video o estirar forma"),
                ("⌘ + arrastrar", "mover foto en el recorte"),
                ("⌥ + arrastrar", "sin imán"),
                ("Flechas", "mover 1 px (⇧ 10 px)"),
                ("⌫", "eliminar"),
                ("⌘D", "duplicar"),
                ("⌘Z / ⇧⌘Z", "deshacer / rehacer"),
                ("⌘ + rueda / pellizco", "zoom"),
                ("Clic derecho", "slides y capa"),
                ("P", "vista previa (← → Esc)"),
            ] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(k).monospace());
                    ui.weak(v);
                });
            }
            ui.add_space(14.0);
            ui.label(egui::RichText::new(format!("Carrusel {}", updater::current_version())).strong());
            ui.horizontal(|ui| {
                let busy = matches!(self.update, Update::Checking | Update::Downloading { .. });
                if ui.add_enabled(!busy, egui::Button::new("Buscar actualizaciones")).clicked() {
                    self.check_updates(&ctx, true);
                }
                match &self.update {
                    Update::Checking => {
                        ui.spinner();
                    }
                    Update::UpToDate => {
                        ui.weak("Tienes la última versión.");
                    }
                    _ => {}
                }
            });
            return;
        };

        let slide = self.doc.slide_of(&self.doc.layers[idx]);
        let is_media = self.doc.layers[idx].is_media();
        let what = if self.doc.layers[idx].is_image() { "foto" } else { "video" };
        ui.heading(self.layer_name(&self.doc.layers[idx]));
        ui.weak(format!("{} · en el slide {}", self.doc.layers[idx].kind.label(), slide + 1));
        if self.doc.layers[idx].locked {
            ui.colored_label(Color32::from_rgb(255, 190, 80), "🔒 Capa bloqueada: desbloquéala en el panel Capas para moverla.");
        }
        if self.doc.layers[idx].hidden {
            ui.colored_label(Color32::from_rgb(255, 190, 80), "Capa oculta: no se ve ni se exporta.");
        }
        ui.add_space(8.0);

        // Los controles continuos se agrupan en un solo paso de deshacer.
        let before = self.doc.clone();
        let mut changed = false;

        /// Selector de color con transparencia; devuelve si cambió.
        fn color_row(ui: &mut egui::Ui, label: &str, color: &mut [u8; 4]) -> bool {
            ui.horizontal(|ui| {
                ui.label(label);
                let mut c = Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]);
                let changed = ui.color_edit_button_srgba(&mut c).changed();
                if changed {
                    *color = c.to_srgba_unmultiplied();
                }
                changed
            })
            .inner
        }

        if is_media {
            if let Kind::Video { path, start, muted } = &mut self.doc.layers[idx].kind {
                let info = self.videos.get(path).map(|v| v.info);
                ui.label(egui::RichText::new("Video").strong());
                if let Some(info) = info {
                    ui.weak(format!("{}×{} · {:.1} s{}", info.width, info.height, info.duration, if info.has_audio { " · con sonido" } else { "" }));
                    ui.horizontal(|ui| {
                        ui.label("Empieza en");
                        let max = (info.duration - 0.1).max(0.0);
                        changed |= ui.add(egui::DragValue::new(start).range(0.0..=max).speed(0.05).suffix(" s").max_decimals(1)).changed();
                    });
                    let len = (info.duration - *start).clamp(0.0, video::MAX_SLIDE_SECONDS);
                    ui.weak(format!("Se exporta como MP4 de {len:.1} s (Instagram admite hasta 60 s por slide)."));
                    if info.has_audio {
                        changed |= ui.checkbox(muted, "Sin sonido al exportar").changed();
                    }
                } else {
                    ui.weak("Cargando información del video…");
                }
                if video::ffmpeg().is_none() {
                    ui.colored_label(Color32::from_rgb(255, 190, 80), video::MISSING);
                }
                ui.add_space(8.0);
            }
            ui.label(egui::RichText::new("Encajar").strong());
            ui.horizontal(|ui| {
                ui.label("Ocupar");
                ui.add(egui::DragValue::new(&mut self.span).range(1..=self.doc.slides));
                ui.label(if self.span == 1 { "slide" } else { "slides" });
            });
            let rect = self.doc.slides_rect(slide, self.span);
            ui.horizontal(|ui| {
                if ui.button("Rellenar").on_hover_text("Cubre el área recortando lo que sobre").clicked() {
                    self.fit_selected(rect, true);
                }
                if ui.button("Encajar").on_hover_text(format!("La {what} completa, sin recortar")).clicked() {
                    self.fit_selected(rect, false);
                }
            });
            if ui.button("Panorámica: todo el carrusel").clicked() {
                let all = self.doc.slides_rect(0, self.doc.slides);
                self.fit_selected(all, true);
            }
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Recortar").strong());
            let pan_tip = if sys::WEB { "" } else { " ⌘ + arrastrar la mueve dentro del recorte." };
            ui.weak(format!("Arrastra los tiradores blancos de los lados para recortar la {what}; puedes volver a alargarla hasta su borde original.{pan_tip}"));
            ui.toggle_value(&mut self.pan_crop_mode, format!("Mover la {what} dentro del recorte"))
                .on_hover_text("Mientras esté activo, arrastrar la foto cambia la parte que se ve en lugar de moverla");
            let crop = self.doc.layers[idx].crop;
            if !crop.is_full() {
                let pct = |a: f32| format!("{:.0} %", a * 100.0);
                ui.weak(format!(
                    "Visible: {} del ancho × {} del alto",
                    pct(crop.u1 - crop.u0),
                    pct(crop.v1 - crop.v0)
                ));
            }
            if ui.add_enabled(!crop.is_full(), egui::Button::new("Quitar recorte")).on_hover_text(format!("Vuelve a mostrar la {what} entera")).clicked() {
                self.reset_crop_selected();
            }
            ui.add_space(8.0);
            let l = &mut self.doc.layers[idx];
            let max_r = (l.w.min(l.h) / 2.0).max(1.0);
            l.radius = l.radius.min(max_r);
            ui.horizontal(|ui| {
                ui.label("Esquinas");
                changed |= ui.add(egui::Slider::new(&mut l.radius, 0.0..=max_r).suffix(" px").max_decimals(0)).changed();
            });
            ui.horizontal(|ui| {
                ui.label("Marco");
                let max_b = (l.w.min(l.h) / 4.0).max(1.0);
                changed |= ui.add(egui::Slider::new(&mut l.border, 0.0..=max_b).suffix(" px").max_decimals(0)).changed();
            });
            if l.border > 0.0 {
                changed |= color_row(ui, "Color del marco", &mut l.border_color);
            }
        } else if matches!(self.doc.layers[idx].kind, Kind::Text { .. }) {
            if let Kind::Text { text, size, color, align, background, shadow } = &mut self.doc.layers[idx].kind {
                ui.label(egui::RichText::new("Contenido").strong());
                changed |= ui
                    .add(egui::TextEdit::multiline(text).desired_rows(3).desired_width(f32::INFINITY))
                    .changed();
                ui.horizontal(|ui| {
                    ui.label("Tamaño");
                    changed |= ui.add(egui::DragValue::new(size).range(8.0..=800.0).speed(1.0)).changed();
                });
                changed |= color_row(ui, "Color", color);
                ui.horizontal(|ui| {
                    ui.label("Alinear");
                    for (a, label) in [(TextAlign::Left, "Izq."), (TextAlign::Center, "Centro"), (TextAlign::Right, "Der.")] {
                        changed |= ui.selectable_value(align, a, label).changed();
                    }
                });
                let mut has_bg = background.is_some();
                if ui.checkbox(&mut has_bg, "Recuadro de fondo").changed() {
                    *background = has_bg.then_some([255, 255, 255, 235]);
                    changed = true;
                }
                if let Some(bg) = background {
                    changed |= color_row(ui, "Color del recuadro", bg);
                }
                changed |= ui.checkbox(shadow, "Sombra").changed();
            }
            let (sw, sh) = self.doc.slide_size();
            let (w, h) = (self.doc.layers[idx].w, self.doc.layers[idx].h);
            if ui.button("Centrar en slide").clicked() {
                self.checkpoint();
                let l = &mut self.doc.layers[idx];
                l.x = slide as f32 * sw + (sw - w) / 2.0;
                l.y = (sh - h) / 2.0;
            }
        } else {
            let mut fit_slide = false;
            match &mut self.doc.layers[idx].kind {
                Kind::Emoji { emoji, sticker, style } => {
                    let name = vector::emojis().iter().find(|e| e.emoji == emoji.as_str()).map(|e| e.name).unwrap_or("");
                    ui.weak(name);
                    ui.horizontal(|ui| {
                        ui.label("Estilo");
                        for s in doc::EmojiStyle::ALL {
                            changed |= ui.selectable_value(style, s, s.label()).changed();
                        }
                    });
                    changed |= ui.checkbox(sticker, "Efecto calcomanía (borde blanco y sombra)").changed();
                }
                Kind::Shape { shape, fill, stroke, stroke_w } => {
                    egui::ComboBox::from_label("Forma").selected_text(shape.label()).show_ui(ui, |ui| {
                        for k in ShapeKind::ALL {
                            changed |= ui.selectable_value(shape, k, k.label()).changed();
                        }
                    });
                    let line = *shape == ShapeKind::Line;
                    changed |= color_row(ui, if line { "Color" } else { "Relleno" }, fill);
                    if !line {
                        let mut has_stroke = stroke[3] > 0;
                        if ui.checkbox(&mut has_stroke, "Borde").changed() {
                            stroke[3] = if has_stroke { 255 } else { 0 };
                            changed = true;
                        }
                        if has_stroke {
                            changed |= color_row(ui, "Color del borde", stroke);
                            ui.horizontal(|ui| {
                                ui.label("Grosor");
                                changed |= ui.add(egui::Slider::new(stroke_w, 1.0..=120.0).suffix(" px").max_decimals(0)).changed();
                            });
                        }
                        ui.weak("Estira la forma desde sus lados; ⇧ en la esquina mantiene la proporción.");
                    } else {
                        ui.weak("El grosor de la línea es el alto de su caja: estírala desde arriba o abajo.");
                    }
                }
                Kind::Graphic { source, name, tint, sticker, svg } => {
                    ui.label(name.as_str());
                    if let Some(icon) = source.strip_prefix("iconify:") {
                        let prefix = icon.split(':').next().unwrap_or("");
                        match online::icon_set(prefix) {
                            Some(set) => {
                                ui.weak(format!("{} · licencia {}", set.name, set.license));
                                if set.spdx.starts_with("CC-BY") {
                                    ui.weak("Esta licencia pide mencionar al autor del icono al publicar.");
                                }
                            }
                            None => {
                                ui.weak(format!("Icono de Iconify ({prefix})"));
                            }
                        }
                        ui.hyperlink_to("Ver en Iconify", format!("https://icon-sets.iconify.design/{prefix}/"));
                    } else if source.starts_with("freesvg:") {
                        ui.weak("FreeSVG · dominio público (CC0)");
                    } else {
                        ui.weak("Colección scrapbook de Carrusel");
                    }
                    if svg.contains("currentColor") {
                        let mut c = tint.unwrap_or([40, 40, 40, 255]);
                        if color_row(ui, "Color", &mut c) {
                            *tint = Some(c);
                            changed = true;
                        }
                    }
                    changed |= ui.checkbox(sticker, "Efecto calcomanía (borde blanco y sombra)").changed();
                }
                Kind::Frame { frame, color, thickness } => {
                    egui::ComboBox::from_label("Estilo").selected_text(frame.label()).show_ui(ui, |ui| {
                        for k in FrameKind::ALL {
                            changed |= ui.selectable_value(frame, k, k.label()).changed();
                        }
                    });
                    changed |= color_row(ui, "Color", color);
                    ui.horizontal(|ui| {
                        ui.label("Grosor");
                        changed |= ui.add(egui::Slider::new(thickness, 2.0..=200.0).suffix(" px").max_decimals(0)).changed();
                    });
                    fit_slide = ui.button("Ajustar al slide").clicked();
                }
                _ => {}
            }
            if fit_slide {
                self.checkpoint();
                let rect = self.doc.slides_rect(slide, 1);
                let l = &mut self.doc.layers[idx];
                (l.x, l.y, l.w, l.h) = rect;
            }
        }

        let l = &mut self.doc.layers[idx];
        let mut pct = l.opacity * 100.0;
        ui.horizontal(|ui| {
            ui.label("Opacidad");
            if ui.add(egui::Slider::new(&mut pct, 0.0..=100.0).suffix(" %").max_decimals(0)).changed() {
                l.opacity = pct / 100.0;
                changed = true;
            }
        });
        if changed {
            self.record(before);
        }

        ui.add_space(12.0);
        ui.label(egui::RichText::new("Capa").strong());
        ui.horizontal(|ui| {
            if ui.button("Al frente").clicked() {
                self.reorder(isize::MAX / 2, true);
            }
            if ui.button("Subir").clicked() {
                self.reorder(1, false);
            }
            if ui.button("Bajar").clicked() {
                self.reorder(-1, false);
            }
            if ui.button("Al fondo").clicked() {
                self.reorder(0, true);
            }
        });
        ui.horizontal(|ui| {
            if ui.button("Duplicar").clicked() {
                self.duplicate_selected();
            }
            if ui.button(egui::RichText::new("Eliminar").color(Color32::from_rgb(255, 110, 110))).clicked() {
                self.delete_selected();
            }
        });

        if let Some(Kind::Image { path } | Kind::Video { path, .. }) = self.doc.layers.get(idx).map(|l| &l.kind) {
            ui.add_space(12.0);
            ui.weak(path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            if let Some(err) = self.failed.get(path) {
                ui.colored_label(Color32::from_rgb(255, 110, 110), err);
            }
        }
    }

    /// Pinta el fondo y todas las capas con el origen del lienzo en `origin`
    /// y `zoom` píxeles de pantalla por píxel del lienzo.
    fn paint_layers(&self, painter: &egui::Painter, origin: Pos2, zoom: f32) {
        let to_screen = |x: f32, y: f32| origin + Vec2::new(x, y) * zoom;
        let (cw, ch) = self.doc.canvas_size();
        let [r, g, b, _] = self.doc.background;
        painter.rect_filled(Rect::from_min_size(origin, Vec2::new(cw, ch) * zoom), 0.0, Color32::from_rgb(r, g, b));
        let rgba = |c: [u8; 4], opacity: f32| {
            Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (c[3] as f32 * opacity).round() as u8)
        };
        for l in self.doc.layers.iter().filter(|l| !l.hidden) {
            let rect = Rect::from_min_max(to_screen(l.x, l.y), to_screen(l.x + l.w, l.y + l.h));
            // Los textos con fondo o sombra sobresalen un poco de su caja.
            if !painter.clip_rect().intersects(rect.expand(40.0)) {
                continue;
            }
            let opacity = l.opacity.clamp(0.0, 1.0);
            let radius = (l.radius.clamp(0.0, l.w.min(l.h) / 2.0) * zoom).min(255.0) as u8;
            let c = l.crop;
            let uv = Rect::from_min_max(Pos2::new(c.u0, c.v0), Pos2::new(c.u1, c.v1));
            let tint = Color32::WHITE.gamma_multiply(opacity);
            let media_tex = match &l.kind {
                Kind::Image { path } => Some(self.images.get(path).map(|i| i.tex.id()).ok_or(path)),
                Kind::Video { path, .. } => Some(
                    self.video_tex
                        .get(&l.id)
                        .map(|t| t.id())
                        .or_else(|| self.videos.get(path).map(|v| v.poster.id()))
                        .ok_or(path),
                ),
                _ => None,
            };
            match (&l.kind, media_tex) {
                (_, Some(Ok(tex))) => {
                    painter.add(RectShape::filled(rect, CornerRadius::same(radius), tint).with_texture(tex, uv));
                    if l.border > 0.0 {
                        let t = l.border.min(l.w.min(l.h) / 2.0) * zoom;
                        let stroke = Stroke::new(t, rgba(l.border_color, opacity));
                        painter.rect_stroke(rect, CornerRadius::same(radius), stroke, StrokeKind::Inside);
                    }
                }
                (_, Some(Err(path))) => {
                    painter.rect_filled(rect, 0.0, Color32::from_gray(60));
                    let msg = if self.failed.contains_key(path) { "⚠ no encontrado" } else { "cargando…" };
                    painter.text(rect.center(), Align2::CENTER_CENTER, msg, FontId::proportional(13.0), Color32::LIGHT_GRAY);
                }
                (Kind::Text { text, size, color, align, background, shadow }, None) => {
                    if let Some(bg) = background {
                        let (px, py) = render::text_padding(*size);
                        let bg_rect = Rect::from_min_max(to_screen(l.x - px, l.y - py), to_screen(l.x + l.w + px, l.y + l.h + py));
                        let r = (size * 0.35 * zoom).min(255.0) as u8;
                        painter.rect_filled(bg_rect, CornerRadius::same(r), rgba(*bg, opacity));
                    }
                    let font = FontId::new(size * zoom, FontFamily::Name(CANVAS_FONT.into()));
                    let (x, anchor) = match align {
                        TextAlign::Left => (l.x, Align2::LEFT_TOP),
                        TextAlign::Center => (l.x + l.w / 2.0, Align2::CENTER_TOP),
                        TextAlign::Right => (l.x + l.w, Align2::RIGHT_TOP),
                    };
                    let off = (size * 0.05).max(1.0);
                    for (i, line) in text.split('\n').enumerate() {
                        let p = to_screen(x, l.y + i as f32 * size * LINE_H);
                        if *shadow {
                            let sp = p + Vec2::splat(off * zoom);
                            painter.text(sp, anchor, line, font.clone(), rgba([0, 0, 0, 115], opacity));
                        }
                        painter.text(p, anchor, line, font.clone(), rgba(*color, opacity));
                    }
                }
                (_, None) => {
                    // Emojis, calcomanías, formas y marcos: textura generada por sync_vector_textures.
                    if let Some(vt) = self.vector_tex.get(&l.id) {
                        let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                        painter.image(vt.tex.id(), rect, full, tint);
                    }
                }
            }
            if l.is_video() && self.preview.is_none() {
                // Distintivo para reconocer los videos en el editor.
                let badge = Rect::from_min_size(rect.min + Vec2::new(6.0, 6.0), Vec2::new(26.0, 18.0));
                if painter.clip_rect().contains_rect(badge) {
                    painter.rect_filled(badge, 4.0, Color32::from_black_alpha(160));
                    painter.text(badge.center(), Align2::CENTER_CENTER, "▶", FontId::proportional(11.0), Color32::WHITE);
                }
            }
        }
    }

    /// Genera (o reutiliza) la textura de cada capa vectorial a la resolución
    /// con la que se va a ver; se regenera solo si cambia su contenido o el zoom
    /// cambia lo bastante.
    fn sync_vector_textures(&mut self, ctx: &egui::Context, zoom: f32) {
        use std::hash::{Hash, Hasher};
        let ppp = ctx.pixels_per_point();
        let mut alive = Vec::new();
        for l in &self.doc.layers {
            let Some(svg) = vector::layer_svg(&l.kind, l.w, l.h) else { continue };
            alive.push(l.id);
            // Resolución en escalones de √2 para no regenerar con cada paso de zoom.
            let target = (l.w.max(l.h) * zoom * ppp).clamp(16.0, 2048.0);
            let step = 2f32.powf((target.log2() * 2.0).ceil() / 2.0);
            let scale = (step / l.w.max(l.h)).min(2048.0 / l.w.max(l.h));
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            svg.hash(&mut hasher);
            scale.to_bits().hash(&mut hasher);
            let key = hasher.finish();
            if self.vector_tex.get(&l.id).is_some_and(|v| v.key == key) {
                continue;
            }
            let size = (((l.w * scale).ceil() as u32).max(1), ((l.h * scale).ceil() as u32).max(1));
            if let Some(img) = vector::rasterize(&svg, scale, (0.0, 0.0), size) {
                let color = egui::ColorImage::from_rgba_unmultiplied([size.0 as usize, size.1 as usize], img.as_raw());
                match self.vector_tex.get_mut(&l.id) {
                    Some(vt) => {
                        vt.tex.set(color, egui::TextureOptions::LINEAR);
                        vt.key = key;
                    }
                    None => {
                        let tex = ctx.load_texture(format!("vector-{}", l.id), color, egui::TextureOptions::LINEAR);
                        self.vector_tex.insert(l.id, VectorTex { key, tex });
                    }
                }
            }
        }
        self.vector_tex.retain(|id, _| alive.contains(id));
    }

    /// Mantiene un reproductor por cada capa de video (si están activados) y
    /// sube a la GPU el último fotograma de cada uno.
    fn sync_video_players(&mut self, ctx: &egui::Context) {
        let wanted: Vec<(u64, PathBuf, f32)> = if self.play_videos && video::ffmpeg().is_some() {
            self.doc
                .layers
                .iter()
                .filter_map(|l| match &l.kind {
                    Kind::Video { path, start, .. } if !l.hidden && self.videos.contains_key(path) => Some((l.id, path.clone(), *start)),
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };
        self.players.retain(|id, _| wanted.iter().any(|(w, _, _)| w == id));
        self.video_tex.retain(|id, _| wanted.iter().any(|(w, _, _)| w == id));
        // No relanzar el video mientras se arrastra el control de «inicio».
        let settling = ctx.dragged_id().is_some();
        for (id, path, start) in wanted {
            let stale = self.players.get(&id).is_some_and(|p| p.path != path || (p.start - start).abs() > 0.01);
            if (stale && !settling) || !self.players.contains_key(&id) {
                let size = video::fit_size(&self.videos[&path].info, 640);
                let repaint = ctx.clone();
                match video::Player::new(&path, start, size, move || repaint.request_repaint()) {
                    Ok(p) => {
                        self.players.insert(id, p);
                    }
                    Err(e) => self.status = e,
                }
            }
            if let Some(frame) = self.players.get(&id).and_then(|p| p.take_frame()) {
                let color = egui::ColorImage::from_rgba_unmultiplied([frame.width() as usize, frame.height() as usize], frame.as_raw());
                match self.video_tex.get_mut(&id) {
                    Some(t) => t.set(color, egui::TextureOptions::LINEAR),
                    None => {
                        let tex = ctx.load_texture(format!("video-{id}"), color, egui::TextureOptions::LINEAR);
                        self.video_tex.insert(id, tex);
                    }
                }
            }
        }
    }

    /// Vista previa tipo Instagram: un slide a la vez, deslizable.
    fn preview(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let n = self.doc.slides;
        let (sw, sh) = self.doc.slide_size();
        let area = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(area, Sense::click_and_drag());
        let painter = ui.painter_at(area);
        let Some(pv) = &mut self.preview else { return };
        pv.index = pv.index.min(n - 1);

        // Tarjeta del post: cabecera, slide y puntos.
        let (header, footer) = (52.0, 46.0);
        let scale = ((area.width() - 80.0) / sw).min((area.height() - 60.0 - header - footer) / sh).clamp(0.02, 1.0);
        let slide_size = Vec2::new(sw, sh) * scale;
        let card = Rect::from_center_size(area.center(), Vec2::new(slide_size.x, slide_size.y + header + footer));
        let slide_rect = Rect::from_min_size(card.min + Vec2::new(0.0, header), slide_size);

        // Arrastre para deslizar y clic en los lados para pasar de slide.
        if resp.dragged() {
            pv.drag_dx += resp.drag_delta().x;
        }
        let live = (pv.index as f32 - pv.drag_dx / slide_size.x).clamp(-0.25, n as f32 - 0.75);
        if resp.drag_stopped() {
            let threshold = slide_size.x * 0.15;
            if pv.drag_dx < -threshold {
                pv.index = (pv.index + 1).min(n - 1);
            } else if pv.drag_dx > threshold {
                pv.index = pv.index.saturating_sub(1);
            }
            pv.pos = live;
            pv.drag_dx = 0.0;
        } else if resp.clicked() {
            if let Some(p) = resp.interact_pointer_pos() {
                if p.x < slide_rect.center().x {
                    pv.index = pv.index.saturating_sub(1);
                } else {
                    pv.index = (pv.index + 1).min(n - 1);
                }
            }
        }
        if resp.dragged() {
            pv.pos = live;
        } else {
            let dt = ctx.input(|i| i.stable_dt).min(0.1);
            let target = pv.index as f32;
            pv.pos += (target - pv.pos) * (1.0 - (-dt * 14.0).exp());
            if (target - pv.pos).abs() < 0.001 {
                pv.pos = target;
            } else {
                ctx.request_repaint();
            }
        }
        let (pos, index) = (pv.pos, pv.index);
        self.sync_vector_textures(&ctx, scale);

        painter.rect_filled(card.expand(1.0), 14.0, Color32::from_gray(10));
        let avatar = card.min + Vec2::new(26.0, header / 2.0);
        painter.circle_filled(avatar, 15.0, ACCENT);
        painter.circle_stroke(avatar, 17.5, Stroke::new(1.5, Color32::from_rgb(255, 170, 60)));
        painter.text(avatar + Vec2::new(26.0, 0.0), Align2::LEFT_CENTER, self.doc_name(), FontId::proportional(14.0), Color32::WHITE);
        painter.text(Pos2::new(card.max.x - 16.0, avatar.y), Align2::RIGHT_CENTER, "•••", FontId::proportional(14.0), Color32::GRAY);

        let clipped = painter.with_clip_rect(slide_rect);
        self.paint_layers(&clipped, slide_rect.min - Vec2::new(pos * slide_size.x, 0.0), scale);

        // Contador «2/5» arriba a la derecha.
        let badge_text = format!("{}/{}", index + 1, n);
        let badge = Rect::from_min_size(Pos2::new(slide_rect.max.x - 52.0, slide_rect.min.y + 12.0), Vec2::new(40.0, 24.0));
        painter.rect_filled(badge, 12.0, Color32::from_black_alpha(170));
        painter.text(badge.center(), Align2::CENTER_CENTER, badge_text, FontId::proportional(12.0), Color32::WHITE);

        // Flechas laterales al pasar el ratón.
        if resp.hovered() {
            for (show, x, glyph) in [
                (index > 0, slide_rect.min.x + 22.0, "‹"),
                (index + 1 < n, slide_rect.max.x - 22.0, "›"),
            ] {
                if show {
                    let c = Pos2::new(x, slide_rect.center().y);
                    painter.circle_filled(c, 13.0, Color32::from_white_alpha(210));
                    painter.text(c - Vec2::new(0.0, 1.5), Align2::CENTER_CENTER, glyph, FontId::proportional(22.0), Color32::from_gray(30));
                }
            }
        }

        // Puntos de paginación.
        let dots_y = slide_rect.max.y + footer / 2.0;
        // Como en Instagram: como mucho 9 puntos visibles, la ventana sigue al
        // slide actual y los de los extremos se ven más pequeños si hay más.
        let gap = 10.0;
        const WINDOW: u32 = 9;
        let shown = n.min(WINDOW);
        let first = index.saturating_sub(WINDOW / 2).min(n - shown);
        let x0 = slide_rect.center().x - gap * (shown as f32 - 1.0) / 2.0;
        for (k, i) in (first..first + shown).enumerate() {
            let active = i == index;
            let c = if active { Color32::from_rgb(0, 149, 246) } else { Color32::from_gray(90) };
            let at_edge = (k == 0 && first > 0) || (k as u32 == shown - 1 && first + shown < n);
            let r = if active { 3.5 } else if at_edge { 2.0 } else { 3.0 };
            painter.circle_filled(Pos2::new(x0 + k as f32 * gap, dots_y), r, c);
        }
        painter.text(
            Pos2::new(area.center().x, area.max.y - 14.0),
            Align2::CENTER_BOTTOM,
            if sys::WEB { "Desliza para pasar de slide · toca «Editar» para volver" } else { "Arrastra o usa ← → para pasar de slide · Esc o P para volver a editar" },
            FontId::proportional(12.0),
            Color32::from_gray(120),
        );
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let (cw, ch) = self.doc.canvas_size();
        let (sw, _) = self.doc.slide_size();
        let margin = Vec2::new(40.0, 48.0);

        if self.fit_request {
            let avail = ui.available_size();
            let z = ((avail.x - margin.x * 2.0) / cw).min((avail.y - margin.y * 2.0) / ch);
            self.zoom = z.clamp(ZOOM_MIN, ZOOM_MAX);
            self.fit_request = false;
        }
        self.sync_vector_textures(&ctx, self.zoom);

        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            let instant = egui::style::ScrollAnimation::none();
            // Zoom con ⌘+rueda o pellizco, alrededor del puntero o de los dedos:
            // ese punto del lienzo se queda donde estaba. Se aplica antes de
            // reservar el espacio para que el desplazamiento quepa ya.
            let touch = ctx.multi_touch().filter(|t| ui.clip_rect().contains(t.center_pos));
            let over = touch.is_some() || ui.rect_contains_pointer(ui.clip_rect());
            let dz = if over { ctx.input(|i| i.zoom_delta()) } else { 1.0 };
            if dz != 1.0 {
                let old = self.zoom;
                self.zoom = (old * dz).clamp(ZOOM_MIN, ZOOM_MAX);
                let pivot = touch.map(|t| t.center_pos).or_else(|| ctx.pointer_hover_pos());
                if let (Some(p), Some(o)) = (pivot, self.last_origin) {
                    let c = (p - o) / old;
                    ui.scroll_with_delta_animation(-c * (self.zoom - old), instant);
                }
            }
            // Dos dedos: desplazar el lienzo. Si el primer dedo ya movía una
            // capa, ese movimiento se deshace.
            if let Some(t) = touch {
                if let Some(drag) = self.drag.take() {
                    if let Some(l) = self.doc.layer_mut(drag.id) {
                        *l = drag.start;
                    }
                    self.undo.pop();
                }
                self.panning = false;
                ui.scroll_with_delta_animation(t.translation_delta, instant);
            }

            let zoom = self.zoom;
            let size = Vec2::new(cw * zoom, ch * zoom) + margin * 2.0;
            let size = size.max(ui.available_size());
            let (area, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
            let origin = Pos2::new(
                area.min.x + ((area.width() - cw * zoom) / 2.0).max(margin.x),
                area.min.y + ((area.height() - ch * zoom) / 2.0).max(margin.y),
            );
            let to_screen = |x: f32, y: f32| origin + Vec2::new(x, y) * zoom;
            let to_canvas = |p: Pos2| ((p - origin) / zoom).to_pos2();
            let canvas_rect = Rect::from_min_size(origin, Vec2::new(cw, ch) * zoom);
            let painter = ui.painter_at(area);
            self.last_origin = Some(origin);

            // Sombra, fondo y capas (recortadas al lienzo).
            painter.rect_filled(canvas_rect.translate(Vec2::new(0.0, 6.0)).expand(2.0), 4.0, Color32::from_black_alpha(90));
            self.paint_layers(&painter.with_clip_rect(canvas_rect), origin, zoom);

            // Divisiones entre slides y numeración.
            let divider = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 64, 129, 200));
            // Solo los slides que se ven en pantalla, y si los números no caben,
            // uno de cada `every`.
            let slide_px = sw * zoom;
            let view = ui.clip_rect();
            let first = (((view.min.x - origin.x) / slide_px).floor().max(0.0) as u32).min(self.doc.slides);
            let last = (((view.max.x - origin.x) / slide_px).ceil().max(0.0) as u32).min(self.doc.slides);
            let label_w = 8.0 * (self.doc.slides.to_string().len() as f32) + 10.0;
            let every = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000, 10_000]
                .into_iter()
                .find(|&k| k as f32 * slide_px >= label_w)
                .unwrap_or(u32::MAX);
            for i in first..=last {
                let x = i as f32 * sw;
                if i > 0 && i < self.doc.slides {
                    painter.extend(Shape::dashed_line(
                        &[to_screen(x, 0.0), to_screen(x, ch)],
                        divider,
                        6.0,
                        4.0,
                    ));
                }
                if i < self.doc.slides && (i + 1) % every == 0 {
                    painter.text(
                        to_screen(x + sw / 2.0, 0.0) - Vec2::new(0.0, 10.0),
                        Align2::CENTER_BOTTOM,
                        format!("{}", i + 1),
                        FontId::proportional(13.0),
                        Color32::GRAY,
                    );
                }
            }

            // Guías del imán.
            for &(vertical, v) in &self.guides {
                let (a, b) = if vertical {
                    (to_screen(v, -20.0 / zoom), to_screen(v, ch + 20.0 / zoom))
                } else {
                    (to_screen(-20.0 / zoom, v), to_screen(cw + 20.0 / zoom, v))
                };
                painter.line_segment([a, b], Stroke::new(1.0, Color32::from_rgb(0, 200, 255)));
            }

            // Selección. Con el dedo, tiradores más grandes y más fáciles de atinar.
            let (handle, grab, edge_t) = if sys::WEB { (18.0, 12.0, 9.0) } else { (12.0, 6.0, 6.0) };
            // Las capas bloqueadas u ocultas se ven seleccionadas pero sin tiradores.
            let sel = self.selected.and_then(|id| self.doc.layers.iter().find(|l| l.id == id)).cloned();
            let sel_editable = sel.as_ref().is_some_and(|l| !l.locked && !l.hidden);
            let sel_rect = sel.as_ref().map(|l| Rect::from_min_max(to_screen(l.x, l.y), to_screen(l.x + l.w, l.y + l.h)));
            let sel_is_media = sel.as_ref().is_some_and(|l| l.is_media());
            // Formas y marcos se estiran desde los lados; fotos y videos se recortan.
            let sel_has_edges = sel_editable && sel.as_ref().is_some_and(|l| l.is_media() || l.is_stretchy());

            // Mientras se recorta o se mueve dentro del marco, la foto entera se
            // ve en transparente para saber hasta dónde se puede llegar.
            if let (Some(l), Some(drag), Some(r)) = (&sel, &self.drag, sel_rect) {
                let tex = match &l.kind {
                    Kind::Image { path } => self.images.get(path).map(|i| i.tex.clone()),
                    Kind::Video { path, .. } => {
                        self.video_tex.get(&l.id).cloned().or_else(|| self.videos.get(path).map(|v| v.poster.clone()))
                    }
                    _ => None,
                };
                if let (DragMode::Crop(_) | DragMode::Pan, Some(tex)) = (drag.mode, tex) {
                    {
                        let (fx, fy, fw, fh) = l.full_rect();
                        let full = Rect::from_min_max(to_screen(fx, fy), to_screen(fx + fw, fy + fh));
                        let all = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                        painter.image(tex.id(), full, all, Color32::WHITE.gamma_multiply(0.35));
                        painter.rect_stroke(full, 0.0, Stroke::new(1.0, Color32::from_white_alpha(140)), StrokeKind::Middle);
                        let c = l.crop;
                        let uv = Rect::from_min_max(Pos2::new(c.u0, c.v0), Pos2::new(c.u1, c.v1));
                        painter.image(tex.id(), r, uv, Color32::WHITE);
                    }
                }
            }

            if let (Some(r), false) = (sel_rect, sel_editable) {
                // Bloqueada u oculta: contorno gris discontinuo, sin tiradores.
                let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
                painter.extend(Shape::dashed_line(&pts, Stroke::new(1.5, Color32::from_gray(150)), 6.0, 4.0));
            }
            if let Some(r) = sel_rect.filter(|_| sel_editable) {
                painter.rect_stroke(r, 0.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
                if sel_has_edges {
                    for (_, h) in edge_handles(r, edge_t) {
                        painter.rect_filled(h, 3.0, Color32::WHITE);
                        painter.rect_stroke(h, 3.0, Stroke::new(1.5, ACCENT), StrokeKind::Outside);
                    }
                }
                let h = Rect::from_center_size(r.max, Vec2::splat(handle));
                painter.rect_filled(h, 3.0, ACCENT);
                painter.rect_stroke(h, 3.0, Stroke::new(1.5, Color32::WHITE), StrokeKind::Inside);
            }

            // ----- interacción -----
            let on_corner =
                |p: Pos2| sel_editable && sel_rect.is_some_and(|r| Rect::from_center_size(r.max, Vec2::splat(handle + grab)).contains(p));
            let on_edge = |p: Pos2| -> Option<Edge> {
                let r = sel_rect.filter(|_| sel_has_edges)?;
                edge_handles(r, edge_t).into_iter().find(|(_, h)| h.expand(grab).contains(p)).map(|(e, _)| e)
            };
            let hit = |doc: &Doc, p: Pos2| -> Option<u64> {
                let c = to_canvas(p);
                doc.layers.iter().rev().find(|l| !l.hidden && !l.locked && l.contains(c.x, c.y)).map(|l| l.id)
            };
            let command = ctx.input(|i| i.modifiers.command);

            if resp.hovered() && self.drag.is_none() {
                if let Some(p) = resp.hover_pos() {
                    if on_corner(p) {
                        ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                    } else if let Some(e) = on_edge(p) {
                        ctx.set_cursor_icon(match e {
                            Edge::Left | Edge::Right => egui::CursorIcon::ResizeHorizontal,
                            Edge::Top | Edge::Bottom => egui::CursorIcon::ResizeVertical,
                        });
                    } else if command && sel_is_media && hit(&self.doc, p) == self.selected {
                        ctx.set_cursor_icon(egui::CursorIcon::Grab);
                    }
                }
            }
            if let Some(Drag { mode: DragMode::Pan, .. }) = self.drag {
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            }

            // El slide en el que se hace clic recibe los elementos nuevos.
            if resp.drag_started() || resp.clicked() || resp.secondary_clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let c = to_canvas(p);
                    self.focus_slide = ((c.x / sw).floor().max(0.0) as u32).min(self.doc.slides - 1);
                }
            }
            if resp.drag_started() && touch.is_none() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let corner = on_corner(p);
                    let edge = if corner { None } else { on_edge(p) };
                    let target = if corner || edge.is_some() { self.selected } else { hit(&self.doc, p) };
                    // «Mover dentro del recorte» actúa sobre la foto ya seleccionada.
                    let pan = command || (self.pan_crop_mode && target.is_some() && target == self.selected);
                    self.selected = target;
                    if let Some(id) = target {
                        let start = self.doc.layers.iter().find(|l| l.id == id).unwrap().clone();
                        let mode = match edge {
                            _ if corner => DragMode::Resize,
                            Some(e) if start.is_stretchy() => DragMode::Stretch(e),
                            Some(e) => DragMode::Crop(e),
                            None if pan && start.is_media() => DragMode::Pan,
                            None => DragMode::Move,
                        };
                        self.checkpoint();
                        self.drag = Some(Drag { mode, id, start, start_pointer: to_canvas(p) });
                    } else {
                        // Arrastrar el fondo desplaza el lienzo.
                        self.panning = true;
                    }
                }
            } else if resp.clicked() {
                self.selected = resp.interact_pointer_pos().and_then(|p| hit(&self.doc, p));
            }

            self.guides.clear();
            if let (true, Some(drag), Some(p)) = (resp.dragged(), &self.drag, resp.interact_pointer_pos()) {
                let c = to_canvas(p);
                let d = c - drag.start_pointer;
                let s = &drag.start;
                let use_snap = self.snap && !ctx.input(|i| i.modifiers.alt);
                let threshold = 8.0 / zoom;
                let (sw, _) = self.doc.slide_size();
                let (xs, ys) = self.snap_targets(drag.id);
                let mut new = s.clone();
                match drag.mode {
                    DragMode::Move => {
                        new.x = s.x + d.x;
                        new.y = s.y + d.y;
                        if use_snap {
                            if let Some((t, off)) = snap_axis(new.x, new.w, &xs, threshold) {
                                new.x += off;
                                self.guides.push((true, t));
                            }
                            if let Some((t, off)) = snap_axis(new.y, new.h, &ys, threshold) {
                                new.y += off;
                                self.guides.push((false, t));
                            }
                        }
                    }
                    DragMode::Resize if s.is_stretchy() && !ctx.input(|i| i.modifiers.shift) => {
                        // Formas y marcos: ancho y alto libres (⇧ mantiene la proporción).
                        new.w = (s.w + d.x).max(doc::MIN_CROP);
                        new.h = (s.h + d.y).max(doc::MIN_CROP);
                    }
                    DragMode::Resize => {
                        let aspect = s.w / s.h.max(1.0);
                        let mut w = (s.w + d.x).max(s.w + d.y * aspect).max(20.0);
                        if use_snap {
                            let xs: Vec<f32> = (0..=self.doc.slides).map(|k| k as f32 * sw).collect();
                            if let Some(&t) = xs.iter().find(|&&t| (s.x + w - t).abs() < threshold) {
                                w = t - s.x;
                                self.guides.push((true, t));
                            }
                        }
                        let k = w / s.w;
                        new.w = w;
                        new.h = s.h * k;
                        if let Kind::Text { size, .. } = &mut new.kind {
                            if let Kind::Text { size: s0, .. } = &s.kind {
                                *size = (s0 * k).clamp(8.0, 800.0);
                            }
                        }
                    }
                    DragMode::Crop(edge) | DragMode::Stretch(edge) => {
                        let vertical_line = matches!(edge, Edge::Left | Edge::Right);
                        let (from, delta) = match edge {
                            Edge::Left => (s.x, d.x),
                            Edge::Right => (s.x + s.w, d.x),
                            Edge::Top => (s.y, d.y),
                            Edge::Bottom => (s.y + s.h, d.y),
                        };
                        let mut to = from + delta;
                        let mut guide = None;
                        if use_snap {
                            let targets = if vertical_line { &xs } else { &ys };
                            if let Some(&t) = targets
                                .iter()
                                .filter(|&&t| (to - t).abs() < threshold)
                                .min_by(|a, b| (to - **a).abs().total_cmp(&(to - **b).abs()))
                            {
                                to = t;
                                guide = Some(t);
                            }
                        }
                        if matches!(drag.mode, DragMode::Stretch(_)) {
                            new.stretch_edge(edge, to);
                        } else {
                            new.crop_edge(edge, to);
                        }
                        // Solo se muestra la guía si el borde llegó a ella (no se pasó del límite de la foto).
                        let landed = match edge {
                            Edge::Left => new.x,
                            Edge::Right => new.x + new.w,
                            Edge::Top => new.y,
                            Edge::Bottom => new.y + new.h,
                        };
                        if let Some(t) = guide.filter(|t| (landed - t).abs() < 0.5) {
                            self.guides.push((vertical_line, t));
                        }
                    }
                    DragMode::Pan => new.pan_crop(d.x, d.y),
                }
                let id = drag.id;
                if let Some(l) = self.doc.layer_mut(id) {
                    *l = new;
                }
            }
            if self.panning && resp.dragged() && touch.is_none() {
                ui.scroll_with_delta_animation(resp.drag_delta(), instant);
            }
            if resp.drag_stopped() {
                self.drag = None;
                self.panning = false;
            }

            // Menú contextual: acciones de la capa y del slide bajo el puntero.
            if resp.secondary_clicked() {
                if let Some(p) = resp.interact_pointer_pos() {
                    let c = to_canvas(p);
                    self.menu_slide = ((c.x / sw).floor().max(0.0) as u32).min(self.doc.slides - 1);
                    self.selected = hit(&self.doc, p);
                }
            }
            resp.context_menu(|ui| {
                if self.selected.is_some() {
                    if ui.button("Duplicar").clicked() {
                        self.duplicate_selected();
                        ui.close();
                    }
                    if ui.button("Traer al frente").clicked() {
                        self.reorder(isize::MAX / 2, true);
                        ui.close();
                    }
                    if ui.button("Enviar al fondo").clicked() {
                        self.reorder(0, true);
                        ui.close();
                    }
                    if ui.button("Ocultar capa").clicked() {
                        if let Some(i) = self.selected.and_then(|id| self.doc.index_of(id)) {
                            self.checkpoint();
                            self.doc.layers[i].hidden = true;
                        }
                        ui.close();
                    }
                    if ui.button("Bloquear capa").clicked() {
                        if let Some(i) = self.selected.and_then(|id| self.doc.index_of(id)) {
                            self.checkpoint();
                            self.doc.layers[i].locked = true;
                        }
                        ui.close();
                    }
                    if ui.button("Eliminar capa").clicked() {
                        self.delete_selected();
                        ui.close();
                    }
                    ui.separator();
                }
                let s = self.menu_slide;
                ui.weak(format!("Slide {}", s + 1));
                if ui.button("Insertar slide antes").clicked() {
                    self.insert_slide(s);
                    ui.close();
                }
                if ui.button("Insertar slide después").clicked() {
                    self.insert_slide(s + 1);
                    ui.close();
                }
                if ui.add_enabled(self.doc.slides > 1, egui::Button::new("Eliminar este slide")).clicked() {
                    self.remove_slide(s);
                    ui.close();
                }
                ui.separator();
                if ui.button("Ver en vista previa").clicked() {
                    self.preview = Some(Preview { index: s, pos: s as f32, drag_dx: 0.0 });
                    ui.close();
                }
            });

            if (resp.hovered() || sys::WEB) && self.doc.layers.is_empty() && self.pending == 0 {
                painter.text(
                    canvas_rect.center(),
                    Align2::CENTER_CENTER,
                    if sys::WEB { "Toca «＋ Fotos» para empezar" } else { "Suelta aquí tus fotos" },
                    FontId::proportional(18.0),
                    Color32::from_gray(150),
                );
            }

            // Fotos soltadas sobre la ventana (en iPad, arrastradas desde Fotos o Archivos).
            let dropped: Vec<egui::DroppedFileHandle> = ctx.input(|i| {
                i.raw.dropped_files.iter().filter(|f| !f.path().as_os_str().is_empty() && is_supported(f.path())).cloned().collect()
            });
            if !dropped.is_empty() {
                let pointer = ctx.input(|i| i.pointer.hover_pos());
                // Una sola foto sobre el lienzo: slide vacío → lo rellena; si ya
                // hay foto, se coloca donde la soltaste.
                let place = pointer.filter(|p| canvas_rect.contains(*p) && dropped.len() == 1).map(|p| {
                    let c = to_canvas(p);
                    let slide = ((c.x / sw).floor().max(0.0) as u32).min(self.doc.slides - 1);
                    let occupied = self.doc.layers.iter().any(|l| l.is_media() && self.doc.slide_of(l) == slide);
                    if occupied { Place::At(c.x, c.y) } else { Place::Slide(slide) }
                });
                #[cfg(target_arch = "wasm32")]
                self.web_drop(dropped, place, &ctx);
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let paths: Vec<PathBuf> = dropped.iter().map(|f| f.path().to_path_buf()).collect();
                    match place {
                        Some(place) => self.request_load(paths[0].clone(), place, &ctx),
                        None => self.add_media(paths, &ctx),
                    }
                }
            }
            let hovering_files = ctx.input(|i| !i.raw.hovered_files.is_empty());
            if hovering_files {
                painter.rect_stroke(canvas_rect.expand(4.0), 6.0, Stroke::new(3.0, ACCENT), StrokeKind::Outside);
            }
        });
    }
}

/// Tiradores de recorte en el centro de cada lado del marco (en pantalla),
/// de grosor `t`.
fn edge_handles(r: Rect, t: f32) -> [(Edge, Rect); 4] {
    let long_x = (r.width() * 0.4).clamp(t, 28.0 * t / 6.0);
    let long_y = (r.height() * 0.4).clamp(t, 28.0 * t / 6.0);
    [
        (Edge::Left, Rect::from_center_size(Pos2::new(r.min.x, r.center().y), Vec2::new(t, long_y))),
        (Edge::Right, Rect::from_center_size(Pos2::new(r.max.x, r.center().y), Vec2::new(t, long_y))),
        (Edge::Top, Rect::from_center_size(Pos2::new(r.center().x, r.min.y), Vec2::new(long_x, t))),
        (Edge::Bottom, Rect::from_center_size(Pos2::new(r.center().x, r.max.y), Vec2::new(long_x, t))),
    ]
}

/// Busca el borde (inicio, centro o fin) de un segmento más cercano a alguna guía.
/// Devuelve (guía, desplazamiento a aplicar).
fn snap_axis(pos: f32, len: f32, targets: &[f32], threshold: f32) -> Option<(f32, f32)> {
    let edges = [pos, pos + len / 2.0, pos + len];
    let mut best: Option<(f32, f32)> = None;
    for &t in targets {
        for e in edges {
            let off = t - e;
            if off.abs() < threshold && best.is_none_or(|(_, b)| off.abs() < b.abs()) {
                best = Some((t, off));
            }
        }
    }
    best
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Cerrar la ventana con cambios sin guardar pide confirmación.
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            if self.confirm_discard() {
                self.allow_close = true;
                // Una actualización ya descargada se instala al cerrar.
                self.schedule_install(false);
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        }
        self.poll_updates();

        // Cuando no se arrastra ningún control ni se escribe, termina el gesto
        // y el siguiente cambio vuelve a ser un paso de deshacer nuevo.
        if ctx.dragged_id().is_none() && !ctx.egui_wants_keyboard_input() {
            self.coalesce = false;
        }

        #[cfg(target_arch = "wasm32")]
        self.web_frame(&ctx);
        self.receive_loads(&ctx);
        self.sync_video_players(&ctx);
        self.poll_export(&ctx);
        self.sync_font(&ctx);
        self.shortcuts(&ctx);
        self.sync_text_bounds();

        let title = format!("{}{} — Carrusel", if self.dirty { "• " } else { "" }, self.doc_name());
        #[cfg(target_arch = "wasm32")]
        sys::set_title(&title);
        #[cfg(not(target_arch = "wasm32"))]
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(4.0);
            self.toolbar(ui);
            ui.add_space(4.0);
        });
        if matches!(self.update, Update::Available(_) | Update::Downloading { .. } | Update::Ready { .. } | Update::Failed(_)) {
            egui::Panel::top("update")
                .frame(egui::Frame::new().fill(Color32::from_rgb(52, 36, 46)).inner_margin(egui::Margin::symmetric(10, 6)))
                .show(ui, |ui| self.update_banner(ui));
        }
        #[cfg(target_arch = "wasm32")]
        self.web_banner(ui);
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.pending > 0 || self.exporting.is_some() {
                    ui.spinner();
                }
                if self.pending > 0 {
                    ui.label(format!("Cargando {} foto(s)…", self.pending));
                } else {
                    ui.label(&self.status);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (w, h) = self.doc.format.size();
                    ui.weak(format!("{} × {w}×{h}px", self.doc.slides));
                });
            });
        });
        if self.library.open {
            egui::Panel::left("library")
                .resizable(true)
                .default_size(250.0)
                .size_range(library::PANEL_MIN..=library::PANEL_MAX)
                .show(ui, |ui| {
                ui.add_space(8.0);
                if let Some(action) = self.library.ui(ui) {
                    self.apply_library(action);
                }
            });
        }
        egui::Panel::right("inspector").resizable(true).default_size(280.0).show(ui, |ui| {
            // Capas abajo, como en Photoshop; el inspector ocupa el resto.
            egui::Panel::bottom("layers").resizable(true).default_size(280.0).size_range(120.0..=700.0).show(ui, |ui| {
                self.layers_panel(ui);
            });
            ui.add_space(8.0);
            egui::ScrollArea::vertical().show(ui, |ui| self.inspector(ui));
        });
        egui::CentralPanel::no_frame()
            .frame(egui::Frame::NONE.fill(Color32::from_gray(28)))
            .show(ui, |ui| {
                if self.preview.is_some() {
                    self.preview(ui);
                } else {
                    self.canvas(ui);
                }
            });
    }
}
