//! Catálogo de Google Fonts: buscar, ver cada familia escrita con su propia
//! letra y descargar la elegida. Usa Fontsource, que sirve las mismas
//! fuentes en TTF (lo que entiende el motor de texto) y deja usarlas desde
//! el navegador. Todas tienen licencias libres (OFL o Apache).

use crate::{online, render, sys};
use ab_glyph::FontArc;
use eframe::egui::{self, Color32, TextureHandle, Vec2};
use image::RgbaImage;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

const CATALOG_URL: &str = "https://api.fontsource.org/v1/fonts?type=google";

#[derive(Clone, Deserialize)]
pub struct FontInfo {
    pub id: String,
    pub family: String,
    #[serde(default)]
    subsets: Vec<String>,
    #[serde(default)]
    pub weights: Vec<u16>,
    #[serde(default)]
    styles: Vec<String>,
    #[serde(default)]
    pub category: String,
}

impl FontInfo {
    fn has_italic(&self) -> bool {
        self.styles.iter().any(|s| s == "italic")
    }

    /// El grosor disponible más cercano al pedido.
    fn closest_weight(&self, wanted: u16) -> u16 {
        self.weights.iter().copied().min_by_key(|w| (*w as i32 - wanted as i32).abs()).unwrap_or(400)
    }
}

/// Familias con letras latinas (las que sirven para escribir en español).
pub async fn catalog() -> Result<Vec<FontInfo>, String> {
    let data = online::cached_get("fuentes/catalogo.json", CATALOG_URL, Some(7)).await?;
    let mut list: Vec<FontInfo> =
        serde_json::from_slice(&data).map_err(|_| "El catálogo de fuentes respondió algo inesperado.".to_string())?;
    list.retain(|f| f.subsets.iter().any(|s| s == "latin") && !f.weights.is_empty());
    list.sort_by_key(|f| f.family.to_lowercase());
    Ok(list)
}

/// Descarga una fuente (letras latinas, con tildes y eñe) en TTF.
pub async fn download(id: &str, weight: u16, italic: bool) -> Result<Vec<u8>, String> {
    let style = if italic { "italic" } else { "normal" };
    let url = format!("https://cdn.jsdelivr.net/fontsource/fonts/{id}@latest/latin-{weight}-{style}.ttf");
    online::cached_get(&format!("fuentes/{id}-{weight}-{style}.ttf"), &url, None).await
}

pub const WEIGHTS: [(u16, &str); 9] = [
    (100, "Finísima"),
    (200, "Extrafina"),
    (300, "Fina"),
    (400, "Normal"),
    (500, "Media"),
    (600, "Seminegrita"),
    (700, "Negrita"),
    (800, "Extranegrita"),
    (900, "Black"),
];

fn weight_label(w: u16) -> &'static str {
    WEIGHTS.iter().find(|(v, _)| *v == w).map(|(_, l)| *l).unwrap_or("Normal")
}

const CATEGORIES: [(&str, &str); 5] = [
    ("sans-serif", "Sin serifa"),
    ("serif", "Con serifa"),
    ("display", "Decorativas"),
    ("handwriting", "Manuscritas"),
    ("monospace", "Monoespaciadas"),
];

/// Fuente elegida, lista para usar.
pub struct Picked {
    pub file_name: String,
    pub bytes: Vec<u8>,
}

enum Msg {
    Catalog(Result<Vec<FontInfo>, String>),
    Preview { id: String, image: Option<RgbaImage> },
    Picked(Result<Picked, String>),
}

/// Cuántas muestras se descargan a la vez.
const MAX_INFLIGHT: usize = 6;
const ROW_H: f32 = 46.0;

pub struct FontPicker {
    pub open: bool,
    query: String,
    category: Option<&'static str>,
    weight: u16,
    italic: bool,
    catalog: Option<Result<Vec<FontInfo>, String>>,
    loading: bool,
    previews: HashMap<String, Option<TextureHandle>>,
    requested: HashSet<String>,
    inflight: usize,
    downloading: Option<String>,
    error: Option<String>,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
}

impl Default for FontPicker {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            open: false,
            query: String::new(),
            category: None,
            weight: 400,
            italic: false,
            catalog: None,
            loading: false,
            previews: HashMap::new(),
            requested: HashSet::new(),
            inflight: 0,
            downloading: None,
            error: None,
            tx,
            rx,
        }
    }
}

impl FontPicker {
    fn spawn(&self, ctx: &egui::Context, job: impl std::future::Future<Output = Msg> + sys::MaybeSend + 'static) {
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        sys::spawn(async move {
            let _ = tx.send(job.await);
            ctx.request_repaint();
        });
    }

    fn receive(&mut self, ctx: &egui::Context) -> Option<Picked> {
        let mut picked = None;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Catalog(r) => {
                    self.loading = false;
                    self.catalog = Some(r);
                }
                Msg::Preview { id, image } => {
                    self.inflight = self.inflight.saturating_sub(1);
                    let tex = image.map(|img| {
                        let color = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                        ctx.load_texture(format!("fuente:{id}"), color, egui::TextureOptions::LINEAR)
                    });
                    self.previews.insert(id, tex);
                }
                Msg::Picked(r) => {
                    self.downloading = None;
                    match r {
                        Ok(p) => {
                            self.error = None;
                            picked = Some(p);
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
            }
        }
        picked
    }

    /// Pide la muestra de una familia (su nombre escrito con su letra).
    fn request_preview(&mut self, ctx: &egui::Context, f: &FontInfo) {
        if self.inflight >= MAX_INFLIGHT || !self.requested.insert(f.id.clone()) {
            return;
        }
        self.inflight += 1;
        let (id, family, weight) = (f.id.clone(), f.family.clone(), f.closest_weight(400));
        let size = 24.0 * ctx.pixels_per_point();
        self.spawn(ctx, async move {
            let image = download(&id, weight, false)
                .await
                .ok()
                .and_then(|b| FontArc::try_from_vec(b).ok())
                .map(|font| render::text_image(&font, &family, size, [235, 235, 235, 255]));
            Msg::Preview { id, image }
        });
    }

    fn pick(&mut self, ctx: &egui::Context, f: &FontInfo) {
        let weight = f.closest_weight(self.weight);
        let italic = self.italic && f.has_italic();
        let (id, family) = (f.id.clone(), f.family.clone());
        self.downloading = Some(id.clone());
        self.spawn(ctx, async move {
            let result = download(&id, weight, italic).await.and_then(|bytes| {
                FontArc::try_from_vec(bytes.clone()).map_err(|_| "La fuente descargada no se puede leer.".to_string())?;
                let style = if italic { " cursiva" } else { "" };
                Ok(Picked { file_name: format!("{family} {}{style}.ttf", weight_label(weight)), bytes })
            });
            Msg::Picked(result)
        });
    }

    /// Ventana del catálogo. Devuelve la fuente elegida cuando termina de descargarse.
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<Picked> {
        let picked = self.receive(ctx);
        if picked.is_some() {
            self.open = false;
        }
        if !self.open {
            return picked;
        }
        if self.catalog.is_none() && !self.loading {
            self.loading = true;
            self.spawn(ctx, async { Msg::Catalog(catalog().await) });
        }
        let mut open = self.open;
        egui::Window::new("Google Fonts")
            .open(&mut open)
            .default_size([440.0, 560.0])
            .collapsible(false)
            .show(ctx, |ui| self.window(ui));
        self.open = open && self.open;
        picked
    }

    fn window(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Buscar: Montserrat, Playfair, Pacifico…").desired_width(f32::INFINITY));
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(self.category.is_none(), "Todas").clicked() {
                self.category = None;
            }
            for (id, label) in CATEGORIES {
                if ui.selectable_label(self.category == Some(id), label).clicked() {
                    self.category = Some(id);
                }
            }
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("grosor").selected_text(weight_label(self.weight)).show_ui(ui, |ui| {
                for (w, label) in WEIGHTS {
                    ui.selectable_value(&mut self.weight, w, format!("{label} ({w})"));
                }
            });
            ui.checkbox(&mut self.italic, "Cursiva");
        });
        ui.weak("Toca una familia para usarla en todos los textos. Si no tiene ese grosor, se usa el más cercano.");
        if let Some(e) = &self.error {
            ui.colored_label(Color32::from_rgb(255, 120, 120), e);
        }
        if let Some(id) = &self.downloading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(format!("Descargando {id}…"));
            });
        }
        ui.separator();
        let list: Vec<FontInfo> = match &self.catalog {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Cargando el catálogo…");
                });
                return;
            }
            Some(Err(e)) => {
                ui.colored_label(Color32::from_rgb(255, 120, 120), e.as_str());
                if ui.button("Reintentar").clicked() {
                    self.catalog = None;
                }
                return;
            }
            Some(Ok(all)) => {
                let q = crate::vector::fold(self.query.trim());
                all.iter()
                    .filter(|f| self.category.is_none_or(|c| f.category == c))
                    .filter(|f| q.is_empty() || crate::vector::fold(&f.family).contains(&q))
                    .cloned()
                    .collect()
            }
        };
        if list.is_empty() {
            ui.weak("Ninguna familia coincide.");
            return;
        }
        ui.weak(format!("{} familias", list.len()));
        let mut chosen = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, list.len(), |ui, range| {
            for f in &list[range] {
                if !self.previews.contains_key(&f.id) {
                    self.request_preview(&ctx, f);
                }
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), egui::Sense::click());
                let painter = ui.painter_at(rect);
                if resp.hovered() || self.downloading.as_ref() == Some(&f.id) {
                    painter.rect_filled(rect.shrink(1.0), 4.0, Color32::from_gray(50));
                }
                let text_left = rect.min + Vec2::new(8.0, 4.0);
                match self.previews.get(&f.id) {
                    Some(Some(tex)) => {
                        let ppp = ctx.pixels_per_point();
                        let size = tex.size_vec2() / ppp;
                        let size = if size.x > rect.width() - 16.0 { size * ((rect.width() - 16.0) / size.x) } else { size };
                        let r = egui::Rect::from_min_size(text_left, size);
                        let full = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0));
                        painter.image(tex.id(), r, full, Color32::WHITE);
                    }
                    _ => {
                        painter.text(text_left, egui::Align2::LEFT_TOP, &f.family, egui::FontId::proportional(17.0), Color32::from_gray(200));
                    }
                }
                let cat = CATEGORIES.iter().find(|(id, _)| *id == f.category).map(|(_, l)| *l).unwrap_or("");
                let n = f.weights.len();
                let detail = format!("{cat} · {n} {}{}", if n == 1 { "grosor" } else { "grosores" }, if f.has_italic() { " · cursiva" } else { "" });
                painter.text(rect.left_bottom() + Vec2::new(8.0, -3.0), egui::Align2::LEFT_BOTTOM, detail, egui::FontId::proportional(11.0), Color32::from_gray(130));
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && self.downloading.is_none() {
                    chosen = Some(f.clone());
                }
            }
        });
        if let Some(f) = chosen {
            self.pick(&ctx, &f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Necesita internet: `cargo test fuentes_en_vivo -- --ignored`
    #[test]
    #[ignore]
    fn fuentes_en_vivo() {
        let list = pollster::block_on(catalog()).unwrap();
        assert!(list.len() > 1000, "{}", list.len());
        let f = list.iter().find(|f| f.family == "Montserrat").unwrap();
        assert_eq!(f.closest_weight(650), 600);
        let bytes = pollster::block_on(download(&f.id, 700, true)).unwrap();
        let font = FontArc::try_from_vec(bytes).unwrap();
        for c in "ñáéíóúü¿¡".chars() {
            assert_ne!(ab_glyph::Font::glyph_id(&font, c).0, 0, "falta {c}");
        }
        let img = render::text_image(&font, "Montserrat", 40.0, [0, 0, 0, 255]);
        assert!(img.width() > 100 && img.pixels().any(|p| p[3] > 0));
    }
}
