//! Panel «Biblioteca»: emojis y calcomanías, stickers scrapbook, iconos
//! (Iconify), ilustraciones (FreeSVG), formas, marcos y estilos de texto.

use crate::doc::{EmojiStyle, FrameKind, Kind, ShapeKind, TextAlign};
use crate::{online, scrapbook, sys, vector};
use std::sync::mpsc;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, TextureHandle, Vec2};
use std::collections::HashMap;
use std::future::Future;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Emoji,
    Stickers,
    Icons,
    Illustrations,
    Shapes,
    Frames,
    Text,
}

pub struct TextPreset {
    pub label: &'static str,
    pub text: &'static str,
    pub size: f32,
    pub color: [u8; 4],
    pub align: TextAlign,
    pub background: Option<[u8; 4]>,
    pub shadow: bool,
}

pub const TEXT_PRESETS: [TextPreset; 6] = [
    TextPreset { label: "Título", text: "Título", size: 150.0, color: [20, 20, 20, 255], align: TextAlign::Center, background: None, shadow: false },
    TextPreset { label: "Subtítulo", text: "Subtítulo", size: 84.0, color: [40, 40, 40, 255], align: TextAlign::Center, background: None, shadow: false },
    TextPreset { label: "Párrafo", text: "Escribe aquí tu texto.\nPuede tener varias líneas.", size: 48.0, color: [30, 30, 30, 255], align: TextAlign::Left, background: None, shadow: false },
    TextPreset { label: "Etiqueta", text: "NUEVO", size: 60.0, color: [255, 255, 255, 255], align: TextAlign::Center, background: Some([255, 64, 129, 255]), shadow: false },
    TextPreset { label: "Resaltado", text: "Texto destacado", size: 64.0, color: [20, 20, 20, 255], align: TextAlign::Center, background: Some([255, 255, 255, 235]), shadow: false },
    TextPreset { label: "Sobre foto", text: "Sobre la foto", size: 110.0, color: [255, 255, 255, 255], align: TextAlign::Center, background: None, shadow: true },
];

/// Lo que el usuario eligió en la biblioteca.
pub enum Action {
    Emoji(&'static str, bool, EmojiStyle),
    Shape(ShapeKind),
    Frame(FrameKind),
    Text(&'static TextPreset),
    /// Icono, ilustración o sticker (SVG ya limpio).
    Graphic { svg: String, source: String, name: String, tint: Option<[u8; 4]> },
}

/// Respuestas de las búsquedas en línea (llegan desde otros hilos).
enum Online {
    Icons { query: String, start: usize, result: Result<(Vec<String>, bool, HashMap<String, String>), String> },
    Login(Result<String, String>),
    Free { query: String, page: u32, result: Result<(Vec<online::FreeSvgItem>, bool), String> },
    /// Miniatura de una ilustración; en la web viene del propio SVG (que
    /// también se guarda, así añadirla es inmediato).
    FreeThumb { id: u64, image: Option<image::RgbaImage>, svg: Option<String> },
    FreeSvg { result: Result<(String, String, u64), String> },
}

#[derive(Default)]
struct IconSearch {
    input: String,
    query: String,
    names: Vec<String>,
    svgs: HashMap<String, String>,
    more: bool,
    loading: bool,
    error: Option<String>,
}

#[derive(Default)]
struct FreeSearch {
    token: Option<String>,
    email: String,
    password: String,
    input: String,
    query: String,
    items: Vec<online::FreeSvgItem>,
    page: u32,
    more: bool,
    loading: bool,
    downloading: Option<u64>,
    error: Option<String>,
    thumbs: HashMap<u64, TextureHandle>,
    /// SVG ya descargados y limpios (en la web, junto con la miniatura).
    svgs: HashMap<u64, (String, String)>,
    /// Miniaturas que no se pudieron descargar (para no mostrar «cargando» siempre).
    failed_thumbs: std::collections::HashSet<u64>,
}

pub struct Library {
    pub open: bool,
    tab: Tab,
    query: String,
    group: Option<&'static str>,
    sticker: bool,
    style: EmojiStyle,
    thumbs: HashMap<String, TextureHandle>,
    scrap_query: String,
    scrap_category: Option<&'static str>,
    icons: IconSearch,
    icon_tint: [u8; 4],
    free: FreeSearch,
    tx: mpsc::Sender<Online>,
    rx: mpsc::Receiver<Online>,
}

impl Default for Library {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            open: true,
            tab: Tab::Emoji,
            query: String::new(),
            group: Some(vector::GROUPS[0]),
            sticker: false,
            style: EmojiStyle::Twemoji,
            thumbs: HashMap::new(),
            scrap_query: String::new(),
            scrap_category: Some(scrapbook::CATEGORIES[0]),
            icons: IconSearch::default(),
            icon_tint: [40, 40, 40, 255],
            free: FreeSearch { token: online::freesvg_token(), ..Default::default() },
            tx,
            rx,
        }
    }
}

/// Miniaturas nuevas por fotograma, para que la lista no se trabe al abrirla.
const THUMBS_PER_FRAME: usize = 60;
/// Ancho mínimo de cada mosaico de formas y marcos.
const TILE_MIN_W: f32 = 84.0;
/// Límites del ancho del panel: así ningún contenido lo puede ensanchar.
pub const PANEL_MIN: f32 = 220.0;
pub const PANEL_MAX: f32 = 420.0;

impl Library {
    fn thumb(&mut self, ctx: &egui::Context, key: String, kind: &Kind, size: (f32, f32), budget: &mut usize) -> Option<TextureHandle> {
        self.thumb_svg(ctx, key, || vector::layer_svg(kind, size.0, size.1), size, budget)
    }

    /// Miniatura de un SVG cualquiera, generada una sola vez y guardada.
    fn thumb_svg(
        &mut self,
        ctx: &egui::Context,
        key: String,
        svg: impl FnOnce() -> Option<String>,
        (w, h): (f32, f32),
        budget: &mut usize,
    ) -> Option<TextureHandle> {
        if let Some(t) = self.thumbs.get(&key) {
            return Some(t.clone());
        }
        if *budget == 0 {
            ctx.request_repaint();
            return None;
        }
        *budget -= 1;
        let px = 2.0 * ctx.pixels_per_point();
        let size = (((w * px) as u32).max(1), ((h * px) as u32).max(1));
        let svg = svg()?;
        let img = vector::rasterize(&svg, px, (0.0, 0.0), size)?;
        let color = egui::ColorImage::from_rgba_unmultiplied([size.0 as usize, size.1 as usize], img.as_raw());
        let tex = ctx.load_texture(key.clone(), color, egui::TextureOptions::LINEAR);
        self.thumbs.insert(key, tex.clone());
        Some(tex)
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        self.receive(ui.ctx(), &mut action);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
            for (tab, label) in [
                (Tab::Emoji, "Emojis"),
                (Tab::Stickers, "Stickers"),
                (Tab::Icons, "Iconos"),
                (Tab::Illustrations, "Ilustraciones"),
                (Tab::Shapes, "Formas"),
                (Tab::Frames, "Marcos"),
                (Tab::Text, "Textos"),
            ] {
                ui.selectable_value(&mut self.tab, tab, label);
            }
        });
        ui.separator();
        match self.tab {
            Tab::Emoji => self.emoji_tab(ui, &mut action),
            Tab::Stickers => self.stickers_tab(ui, &mut action),
            Tab::Icons => self.icons_tab(ui, &mut action),
            Tab::Illustrations => self.illustrations_tab(ui, &mut action),
            Tab::Shapes => self.shapes_tab(ui, &mut action),
            Tab::Frames => self.frames_tab(ui, &mut action),
            Tab::Text => {
                ui.weak("Haz clic para añadir el estilo al slide actual.");
                ui.add_space(4.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    for p in &TEXT_PRESETS {
                        let fg = Color32::from_rgba_unmultiplied(p.color[0], p.color[1], p.color[2], p.color[3]);
                        let mut text = egui::RichText::new(p.label).size(15.0 + p.size / 20.0).color(fg);
                        if let Some(bg) = p.background {
                            text = text.background_color(Color32::from_rgba_unmultiplied(bg[0], bg[1], bg[2], bg[3]));
                        }
                        let fill = if p.shadow { Color32::from_gray(90) } else { Color32::from_gray(235) };
                        // Ancho exacto del panel: el botón nunca lo ensancha.
                        let size = Vec2::new(ui.available_width(), 44.0);
                        if ui.add_sized(size, egui::Button::new(text).fill(fill).truncate()).clicked() {
                            action = Some(Action::Text(p));
                        }
                    }
                });
            }
        }
        action
    }

    fn emoji_tab(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Buscar: corazón, fuego, gato…").desired_width(f32::INFINITY));
        ui.horizontal(|ui| {
            ui.label("Estilo");
            for s in EmojiStyle::ALL {
                ui.selectable_value(&mut self.style, s, s.label());
            }
        });
        ui.checkbox(&mut self.sticker, "Como calcomanía (borde blanco y sombra)");
        let searching = !self.query.trim().is_empty();
        if !searching {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                for g in vector::GROUPS {
                    if ui.selectable_label(self.group == Some(g), g).clicked() {
                        self.group = Some(g);
                    }
                }
            });
        }
        ui.separator();
        let words: Vec<String> = vector::fold(&self.query).split_whitespace().map(String::from).collect();
        let list: Vec<&'static vector::EmojiEntry> = vector::emojis()
            .iter()
            .filter(|e| {
                if searching {
                    words.iter().all(|w| e.search.contains(w.as_str()))
                } else {
                    Some(e.group) == self.group
                }
            })
            .collect();
        if list.is_empty() {
            ui.weak("Nada coincide con la búsqueda.");
            return;
        }
        let cell = 40.0;
        let cols = ((ui.available_width() / cell).floor() as usize).max(1);
        let rows = list.len().div_ceil(cols);
        let ctx = ui.ctx().clone();
        let mut budget = THUMBS_PER_FRAME;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, cell, rows, |ui, range| {
            for row in range {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    for e in list.iter().skip(row * cols).take(cols) {
                        let kind = Kind::Emoji { emoji: e.emoji.into(), sticker: false, style: self.style };
                        let key = format!("e:{:?}:{}", self.style, e.emoji);
                        let tex = self.thumb(&ctx, key, &kind, (28.0, 28.0), &mut budget);
                        let resp = match tex {
                            Some(t) => ui.add_sized(
                                [cell, cell],
                                egui::Button::image(egui::Image::from_texture((t.id(), Vec2::splat(28.0)))).frame(false),
                            ),
                            None => ui.add_sized([cell, cell], egui::Label::new("·")),
                        };
                        if resp.on_hover_text(e.name).clicked() {
                            *action = Some(Action::Emoji(e.emoji, self.sticker, self.style));
                        }
                    }
                });
            }
        });
    }

    // ----- búsquedas en línea --------------------------------------------

    fn spawn(&self, ctx: &egui::Context, job: impl Future<Output = Online> + sys::MaybeSend + 'static) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        sys::spawn(async move {
            let _ = tx.send(job.await);
            ctx.request_repaint();
        });
    }

    fn receive(&mut self, ctx: &egui::Context, action: &mut Option<Action>) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Online::Icons { query, start, result } => {
                    if query != self.icons.query {
                        continue; // respuesta de una búsqueda anterior
                    }
                    self.icons.loading = false;
                    match result {
                        Ok((names, more, svgs)) => {
                            if start == 0 {
                                self.icons.names.clear();
                            }
                            self.icons.names.extend(names);
                            self.icons.svgs.extend(svgs);
                            self.icons.more = more;
                            self.icons.error = None;
                        }
                        Err(e) => self.icons.error = Some(e),
                    }
                }
                Online::Login(result) => {
                    self.free.loading = false;
                    match result {
                        Ok(token) => {
                            self.free.token = Some(token);
                            self.free.error = None;
                        }
                        Err(e) => self.free.error = Some(e),
                    }
                }
                Online::Free { query, page, result } => {
                    if query != self.free.query {
                        continue;
                    }
                    self.free.loading = false;
                    match result {
                        Ok((items, more)) => {
                            if page == 1 {
                                self.free.items.clear();
                            }
                            // Miniaturas de esta página, en 5 tandas a la vez.
                            let token = self.free.token.clone().unwrap_or_default();
                            for chunk in items.chunks(5) {
                                let (tx, c, chunk, token) = (self.tx.clone(), ctx.clone(), chunk.to_vec(), token.clone());
                                sys::spawn(async move {
                                    for item in chunk {
                                        let (image, svg) = free_thumb(&token, &item).await;
                                        let _ = tx.send(Online::FreeThumb { id: item.id, image, svg });
                                        c.request_repaint();
                                    }
                                });
                            }
                            self.free.items.extend(items);
                            self.free.page = page;
                            self.free.more = more;
                            self.free.error = None;
                        }
                        Err(e) => {
                            if e.contains("caducó") {
                                online::freesvg_logout();
                                self.free.token = None;
                            }
                            self.free.error = Some(e);
                        }
                    }
                }
                Online::FreeThumb { id, image, svg } => {
                    if let (Some(svg), Some(item)) = (svg, self.free.items.iter().find(|i| i.id == id)) {
                        self.free.svgs.insert(id, (svg, item_name(item)));
                    }
                    if image.is_none() {
                        self.free.failed_thumbs.insert(id);
                    }
                    if let Some(img) = image {
                        let img = image::imageops::thumbnail(&img, 128, 128 * img.height() / img.width().max(1));
                        let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                        self.free.thumbs.insert(id, ctx.load_texture(format!("fs:{id}"), color, egui::TextureOptions::LINEAR));
                    }
                }
                Online::FreeSvg { result } => {
                    self.free.downloading = None;
                    match result {
                        Ok((svg, name, id)) => {
                            *action = Some(Action::Graphic { svg, source: format!("freesvg:{id}"), name, tint: None });
                        }
                        Err(e) => self.free.error = Some(e),
                    }
                }
            }
        }
    }

    fn search_icons(&mut self, ctx: &egui::Context, start: usize) {
        let query = self.icons.input.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.icons.query = query.clone();
        self.icons.loading = true;
        self.spawn(ctx, async move {
            let result = match online::search_icons(&query, start).await {
                Ok((names, more)) => {
                    let svgs = online::icon_svgs(&names).await;
                    Ok((names.into_iter().filter(|n| svgs.contains_key(n)).collect(), more, svgs))
                }
                Err(e) => Err(e),
            };
            Online::Icons { query, start, result }
        });
    }

    fn search_free(&mut self, ctx: &egui::Context, page: u32) {
        let (Some(token), query) = (self.free.token.clone(), self.free.input.trim().to_string()) else { return };
        if query.is_empty() {
            return;
        }
        self.free.query = query.clone();
        self.free.loading = true;
        self.spawn(ctx, async move {
            let result = online::freesvg_search(&token, &query, page).await;
            Online::Free { query, page, result }
        });
    }

    // ----- pestañas -------------------------------------------------------

    fn stickers_tab(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        ui.add(egui::TextEdit::singleline(&mut self.scrap_query).hint_text("Buscar: cinta, corazón, flor, clip…").desired_width(f32::INFINITY));
        let searching = !self.scrap_query.trim().is_empty();
        if !searching {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                for c in scrapbook::CATEGORIES {
                    if ui.selectable_label(self.scrap_category == Some(c), c).clicked() {
                        self.scrap_category = Some(c);
                    }
                }
            });
        }
        ui.weak("Colección propia de Carrusel, estilo scrapbook.");
        ui.separator();
        let words: Vec<String> = vector::fold(&self.scrap_query).split_whitespace().map(String::from).collect();
        let list: Vec<&'static scrapbook::Sticker> = scrapbook::stickers()
            .iter()
            .filter(|s| if searching { words.iter().all(|w| s.search.contains(w.as_str())) } else { Some(s.category) == self.scrap_category })
            .collect();
        if list.is_empty() {
            ui.weak("Nada coincide con la búsqueda.");
            return;
        }
        let ctx = ui.ctx().clone();
        let mut budget = THUMBS_PER_FRAME / 3;
        let picked = grid(ui, list.len(), 72.0, |ui, i| {
            let s = list[i];
            let (w, h) = fit_box(vector::svg_aspect(&s.svg), 60.0);
            let tex = self.thumb_svg(&ctx, format!("sb:{}", s.id), || graphic_svg(&s.svg, None, w, h), (w, h), &mut budget);
            thumb_button(ui, tex.as_ref(), 72.0, (w, h)).on_hover_text(&s.name).clicked()
        });
        if let Some(i) = picked {
            let s = list[i];
            *action = Some(Action::Graphic { svg: s.svg.clone(), source: format!("scrapbook:{}", s.id), name: s.name.clone(), tint: None });
        }
    }

    fn icons_tab(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            let resp = ui.add(egui::TextEdit::singleline(&mut self.icons.input).hint_text("Buscar: casa, estrella, cámara…").desired_width(ui.available_width() - 70.0));
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.add_enabled(!self.icons.loading, egui::Button::new("Buscar")).clicked() || enter {
                self.search_icons(&ctx, 0);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Color");
            let mut c = Color32::from_rgba_unmultiplied(self.icon_tint[0], self.icon_tint[1], self.icon_tint[2], self.icon_tint[3]);
            if ui.color_edit_button_srgba(&mut c).changed() {
                self.icon_tint = c.to_srgba_unmultiplied();
            }
            ui.weak("(iconos de un color)");
        });
        ui.weak("≈475 000 iconos de Iconify con licencias libres. Puedes buscar en español.");
        if let Some(e) = &self.icons.error {
            ui.colored_label(Color32::from_rgb(255, 120, 120), e);
        }
        if self.icons.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak("Buscando…");
            });
        }
        if self.icons.more && !self.icons.loading && ui.button("Más resultados").clicked() {
            let start = self.icons.names.len();
            self.search_icons(&ctx, start);
        }
        ui.separator();
        if self.icons.names.is_empty() {
            if !self.icons.loading && !self.icons.query.is_empty() {
                ui.weak("Sin resultados. Prueba otra palabra (en inglés también funciona).");
            }
            return;
        }
        let names = self.icons.names.clone();
        let mut budget = THUMBS_PER_FRAME;
        let picked = grid(ui, names.len(), 44.0, |ui, i| {
            let name = &names[i];
            let svg = self.icons.svgs.get(name).cloned().unwrap_or_default();
            // Miniaturas en gris claro para que se vean sobre el panel oscuro.
            let tex = self.thumb_svg(&ctx, format!("ic:{name}"), || graphic_svg(&svg, Some([225, 225, 225, 255]), 30.0, 30.0), (30.0, 30.0), &mut budget);
            let set = online::icon_set(name.split(':').next().unwrap_or("")).map(|s| format!("{} · {}", s.name, s.license));
            thumb_button(ui, tex.as_ref(), 44.0, (30.0, 30.0)).on_hover_text(format!("{name}\n{}", set.unwrap_or_default())).clicked()
        });
        if let Some(i) = picked {
            let name = names[i].clone();
            if let Some(svg) = self.icons.svgs.get(&name).and_then(|s| vector::sanitize_svg(s)) {
                let tint = svg.contains("currentColor").then_some(self.icon_tint);
                *action = Some(Action::Graphic { svg, source: format!("iconify:{name}"), name, tint });
            }
        }
    }

    /// Las ilustraciones se añaden al terminar de descargarse (ver `receive`).
    fn illustrations_tab(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        let ctx = ui.ctx().clone();
        ui.weak("Ilustraciones de FreeSVG.org: dominio público (CC0), libres para cualquier uso.");
        if let Some(e) = &self.free.error {
            ui.colored_label(Color32::from_rgb(255, 120, 120), e);
        }
        if self.free.token.is_none() {
            ui.add_space(6.0);
            ui.label("FreeSVG pide una cuenta gratuita para usar su catálogo. Entra con la tuya una sola vez:");
            ui.add(egui::TextEdit::singleline(&mut self.free.email).hint_text("Correo de FreeSVG").desired_width(f32::INFINITY));
            ui.add(egui::TextEdit::singleline(&mut self.free.password).password(true).hint_text("Contraseña").desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                let ready = !self.free.email.trim().is_empty() && !self.free.password.is_empty() && !self.free.loading;
                if ui.add_enabled(ready, egui::Button::new("Entrar")).clicked() {
                    let (email, password) = (self.free.email.trim().to_string(), std::mem::take(&mut self.free.password));
                    self.free.loading = true;
                    self.spawn(&ctx, async move { Online::Login(online::freesvg_login(&email, &password).await) });
                }
                if self.free.loading {
                    ui.spinner();
                }
                ui.hyperlink_to("Crear cuenta", "https://freesvg.org/register");
            });
            ui.weak("La contraseña solo se envía a freesvg.org; Carrusel guarda únicamente la sesión.");
            return;
        }
        ui.horizontal(|ui| {
            let resp = ui.add(egui::TextEdit::singleline(&mut self.free.input).hint_text("Buscar: flores, marco vintage…").desired_width(ui.available_width() - 70.0));
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.add_enabled(!self.free.loading, egui::Button::new("Buscar")).clicked() || enter {
                self.search_free(&ctx, 1);
            }
        });
        ui.horizontal(|ui| {
            if self.free.loading || self.free.downloading.is_some() {
                ui.spinner();
                ui.weak(if self.free.downloading.is_some() { "Descargando…" } else { "Buscando…" });
            }
            if self.free.more && !self.free.loading && ui.button("Más resultados").clicked() {
                let next = self.free.page + 1;
                self.search_free(&ctx, next);
            }
            if ui.small_button("Cerrar sesión").clicked() {
                online::freesvg_logout();
                self.free = FreeSearch::default();
            }
        });
        ui.separator();
        if self.free.items.is_empty() {
            if !self.free.loading && !self.free.query.is_empty() {
                ui.weak("Sin resultados. Prueba otra palabra (en inglés también funciona).");
            }
            return;
        }
        let items = self.free.items.clone();
        let picked = grid(ui, items.len(), 84.0, |ui, i| {
            let id = items[i].id;
            let tex = self.free.thumbs.get(&id);
            let size = tex.map(|t| fit_box(t.aspect_ratio(), 72.0)).unwrap_or((72.0, 72.0));
            let label = if items[i].title.is_empty() { format!("#{id}") } else { items[i].title.clone() };
            let resp = if tex.is_none() && self.free.failed_thumbs.contains(&id) {
                // Sin vista previa, pero se puede añadir igual.
                ui.add_sized([84.0, 84.0], egui::Button::new("Sin vista\nprevia").frame(false))
            } else {
                thumb_button(ui, tex, 84.0, size)
            };
            resp.on_hover_text(label).clicked()
        });
        // Ya descargada (en la web, con su miniatura): se añade al momento.
        if let Some(id) = picked.map(|i| items[i].id) {
            if let Some((svg, name)) = self.free.svgs.get(&id) {
                *action = Some(Action::Graphic { svg: svg.clone(), source: format!("freesvg:{id}"), name: name.clone(), tint: None });
                return;
            }
        }
        if let (Some(i), None, Some(token)) = (picked, self.free.downloading, self.free.token.clone()) {
            let item = items[i].clone();
            self.free.downloading = Some(item.id);
            self.spawn(&ctx, async move {
                let result = match online::freesvg_download(&token, &item.svg).await {
                    Ok(raw) => vector::sanitize_svg(&raw)
                        .ok_or_else(|| "La ilustración no se puede dibujar (SVG no compatible).".to_string())
                        .map(|svg| (svg, item_name(&item), item.id)),
                    Err(e) => Err(e),
                };
                Online::FreeSvg { result }
            });
        }
    }

    /// Mosaicos de tamaño fijo (dibujo arriba, nombre debajo) que llenan
    /// exactamente el ancho del panel, para que nunca lo ensanchen.
    fn tiles<T: Copy>(
        &mut self,
        ui: &mut egui::Ui,
        items: &[T],
        key: impl Fn(T) -> (String, Kind, &'static str),
        thumb: (f32, f32),
    ) -> Option<T> {
        let ctx = ui.ctx().clone();
        let mut budget = THUMBS_PER_FRAME;
        let mut picked = None;
        let gap = 6.0;
        let width = ui.available_width();
        let cols = ((width + gap) / (TILE_MIN_W + gap)).floor().max(1.0) as usize;
        let cell = Vec2::new(((width - gap * (cols as f32 - 1.0)) / cols as f32).floor(), thumb.1 + 34.0);
        for row in items.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for &item in row {
                    let (k, kind, label) = key(item);
                    let (rect, resp) = ui.allocate_exact_size(cell, Sense::click());
                    let visuals = ui.style().interact(&resp);
                    ui.painter().rect_filled(rect, 6.0, visuals.weak_bg_fill);
                    if let Some(t) = self.thumb(&ctx, k, &kind, thumb, &mut budget) {
                        let center = Pos2::new(rect.center().x, rect.min.y + 8.0 + thumb.1 / 2.0);
                        let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                        ui.painter().image(t.id(), Rect::from_center_size(center, Vec2::new(thumb.0, thumb.1)), full, Color32::WHITE);
                    }
                    ui.painter().text(
                        Pos2::new(rect.center().x, rect.max.y - 7.0),
                        Align2::CENTER_BOTTOM,
                        label,
                        FontId::proportional(12.0),
                        visuals.text_color(),
                    );
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        picked = Some(item);
                    }
                }
            });
            ui.add_space(gap);
        }
        picked
    }

    fn shapes_tab(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        ui.weak("Formas con relleno y borde; estíralas desde sus lados.");
        ui.add_space(4.0);
        let preview = |shape: ShapeKind| {
            let kind = Kind::Shape { shape, fill: [255, 64, 129, 255], stroke: [0; 4], stroke_w: 0.0 };
            (format!("s:{shape:?}"), kind, shape.label())
        };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if let Some(s) = self.tiles(ui, &ShapeKind::ALL, preview, (40.0, 40.0)) {
                *action = Some(Action::Shape(s));
            }
        });
    }

    fn frames_tab(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        ui.weak("Si tienes una foto seleccionada, el marco se ajusta a ella; si no, ocupa el slide.");
        ui.add_space(4.0);
        let preview = |frame: FrameKind| {
            let kind = Kind::Frame { frame, color: [235, 235, 235, 255], thickness: 4.0 };
            (format!("f:{frame:?}"), kind, frame.label())
        };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if let Some(f) = self.tiles(ui, &FrameKind::ALL, preview, (40.0, 50.0)) {
                *action = Some(Action::Frame(f));
            }
        });
    }
}

/// Cuadrícula de celdas cuadradas que llena el ancho del panel, con
/// desplazamiento. `cell_ui` dibuja la celda `i` y devuelve si se pulsó.
fn grid(ui: &mut egui::Ui, count: usize, cell: f32, mut cell_ui: impl FnMut(&mut egui::Ui, usize) -> bool) -> Option<usize> {
    let cols = ((ui.available_width() / cell).floor() as usize).max(1);
    let rows = count.div_ceil(cols);
    let mut picked = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, cell, rows, |ui, range| {
        for row in range {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                for i in row * cols..((row + 1) * cols).min(count) {
                    if cell_ui(ui, i) {
                        picked = Some(i);
                    }
                }
            });
        }
    });
    picked
}

fn thumb_button(ui: &mut egui::Ui, tex: Option<&TextureHandle>, cell: f32, size: (f32, f32)) -> egui::Response {
    match tex {
        Some(t) => ui.add_sized([cell, cell], egui::Button::image(egui::Image::from_texture((t.id(), Vec2::new(size.0, size.1)))).frame(false)),
        None => ui.add_sized([cell, cell], egui::Spinner::new().size(12.0)),
    }
}

/// Tamaño que cabe en un cuadrado de `side` con la proporción `aspect`.
fn fit_box(aspect: f32, side: f32) -> (f32, f32) {
    if aspect >= 1.0 {
        (side, (side / aspect).max(4.0))
    } else {
        ((side * aspect).max(4.0), side)
    }
}

fn graphic_svg(svg: &str, tint: Option<[u8; 4]>, w: f32, h: f32) -> Option<String> {
    let kind = Kind::Graphic { svg: svg.to_string(), source: String::new(), name: String::new(), tint, sticker: false };
    vector::layer_svg(&kind, w, h)
}

fn item_name(item: &online::FreeSvgItem) -> String {
    if item.title.is_empty() { format!("Ilustración {}", item.id) } else { item.title.clone() }
}

/// Miniatura de una ilustración de FreeSVG: el PNG público en escritorio. En
/// la web el navegador no deja leer ese PNG (FreeSVG no lo permite desde otras
/// páginas), así que se dibuja el propio SVG, que sí se puede descargar.
#[cfg(not(target_arch = "wasm32"))]
async fn free_thumb(_token: &str, item: &online::FreeSvgItem) -> (Option<image::RgbaImage>, Option<String>) {
    let image = online::get(&item.thumb).await.ok().and_then(|b| image::load_from_memory(&b).ok()).map(|i| i.to_rgba8());
    (image, None)
}

#[cfg(target_arch = "wasm32")]
async fn free_thumb(token: &str, item: &online::FreeSvgItem) -> (Option<image::RgbaImage>, Option<String>) {
    let Some(svg) = online::freesvg_download(token, &item.svg).await.ok().and_then(|raw| vector::sanitize_svg(&raw)) else {
        return (None, None);
    };
    sys::yield_now().await;
    let (w, h) = fit_box(vector::svg_aspect(&svg), 128.0);
    let image = graphic_svg(&svg, None, w, h).and_then(|s| vector::rasterize(&s, 1.0, (0.0, 0.0), (w as u32, h as u32)));
    (image, Some(svg))
}
