//! Panel «Biblioteca»: emojis y calcomanías, formas, marcos y estilos de texto.

use crate::doc::{EmojiStyle, FrameKind, Kind, ShapeKind, TextAlign};
use crate::vector;
use eframe::egui::{self, Color32, TextureHandle, Vec2};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Emoji,
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
}

pub struct Library {
    pub open: bool,
    tab: Tab,
    query: String,
    group: Option<&'static str>,
    sticker: bool,
    style: EmojiStyle,
    thumbs: HashMap<String, TextureHandle>,
}

impl Default for Library {
    fn default() -> Self {
        Self { open: true, tab: Tab::Emoji, query: String::new(), group: Some(vector::GROUPS[0]), sticker: false, style: EmojiStyle::Twemoji, thumbs: HashMap::new() }
    }
}

/// Miniaturas nuevas por fotograma, para que la lista no se trabe al abrirla.
const THUMBS_PER_FRAME: usize = 60;

impl Library {
    fn thumb(&mut self, ctx: &egui::Context, key: String, kind: &Kind, (w, h): (f32, f32), budget: &mut usize) -> Option<TextureHandle> {
        if let Some(t) = self.thumbs.get(&key) {
            return Some(t.clone());
        }
        if *budget == 0 {
            ctx.request_repaint();
            return None;
        }
        *budget -= 1;
        let px = 2.0 * ctx.pixels_per_point();
        let size = ((w * px) as u32, (h * px) as u32);
        let svg = vector::layer_svg(kind, w, h)?;
        let img = vector::rasterize(&svg, px, (0.0, 0.0), size)?;
        let color = egui::ColorImage::from_rgba_unmultiplied([size.0 as usize, size.1 as usize], img.as_raw());
        let tex = ctx.load_texture(key.clone(), color, egui::TextureOptions::LINEAR);
        self.thumbs.insert(key, tex.clone());
        Some(tex)
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.horizontal(|ui| {
            for (tab, label) in [(Tab::Emoji, "Emojis"), (Tab::Shapes, "Formas"), (Tab::Frames, "Marcos"), (Tab::Text, "Textos")] {
                ui.selectable_value(&mut self.tab, tab, label);
            }
        });
        ui.separator();
        match self.tab {
            Tab::Emoji => self.emoji_tab(ui, &mut action),
            Tab::Shapes => self.shapes_tab(ui, &mut action),
            Tab::Frames => self.frames_tab(ui, &mut action),
            Tab::Text => {
                ui.weak("Haz clic para añadir el estilo al slide actual.");
                ui.add_space(4.0);
                for p in &TEXT_PRESETS {
                    let fg = Color32::from_rgba_unmultiplied(p.color[0], p.color[1], p.color[2], p.color[3]);
                    let mut text = egui::RichText::new(p.label).size(15.0 + p.size / 20.0).color(fg);
                    if let Some(bg) = p.background {
                        text = text.background_color(Color32::from_rgba_unmultiplied(bg[0], bg[1], bg[2], bg[3]));
                    }
                    let fill = if p.shadow { Color32::from_gray(90) } else { Color32::from_gray(235) };
                    if ui.add(egui::Button::new(text).fill(fill).min_size(Vec2::new(ui.available_width(), 44.0))).clicked() {
                        action = Some(Action::Text(p));
                    }
                }
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

    fn grid<T: Copy>(
        &mut self,
        ui: &mut egui::Ui,
        items: &[T],
        key: impl Fn(T) -> (String, Kind, &'static str),
        size: (f32, f32),
    ) -> Option<T> {
        let ctx = ui.ctx().clone();
        let mut budget = THUMBS_PER_FRAME;
        let mut picked = None;
        let cols = ((ui.available_width() / (size.0 + 16.0)).floor() as usize).max(1);
        egui::Grid::new(ui.next_auto_id()).spacing(Vec2::new(6.0, 6.0)).show(ui, |ui| {
            for (i, &item) in items.iter().enumerate() {
                let (k, kind, label) = key(item);
                if let Some(t) = self.thumb(&ctx, k, &kind, size, &mut budget) {
                    let img = egui::Image::from_texture((t.id(), Vec2::new(size.0, size.1)));
                    if ui.add(egui::Button::image_and_text(img, label)).clicked() {
                        picked = Some(item);
                    }
                }
                if (i + 1) % cols == 0 {
                    ui.end_row();
                }
            }
        });
        picked
    }

    fn shapes_tab(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        ui.weak("Formas con relleno y borde; estíralas desde sus lados.");
        ui.add_space(4.0);
        let preview = |shape: ShapeKind| {
            let kind = Kind::Shape { shape, fill: [255, 64, 129, 255], stroke: [0; 4], stroke_w: 0.0 };
            (format!("s:{shape:?}"), kind, shape.label())
        };
        egui::ScrollArea::vertical().show(ui, |ui| {
            if let Some(s) = self.grid(ui, &ShapeKind::ALL, preview, (40.0, 40.0)) {
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
        egui::ScrollArea::vertical().show(ui, |ui| {
            if let Some(f) = self.grid(ui, &FrameKind::ALL, preview, (40.0, 50.0)) {
                *action = Some(Action::Frame(f));
            }
        });
    }
}
