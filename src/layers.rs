//! Panel «Capas» al estilo de Photoshop: lista de arriba (frente) a abajo
//! (fondo) con miniatura, nombre, ojo (ocultar) y candado (bloquear).
//! Se reordena arrastrando y se renombra con doble clic.

use crate::doc::{Kind, Layer};
use crate::{vector, App, ACCENT};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, TextureId, Vec2};

#[derive(Default)]
pub struct LayersUi {
    /// Capa que se está arrastrando para cambiarla de posición.
    dragging: Option<u64>,
    /// Capa que se está renombrando y el texto en edición.
    renaming: Option<(u64, String)>,
    focus_rename: bool,
    /// Mostrar solo las capas del slide actual.
    only_slide: bool,
    /// Alto real de los botones de abajo en el fotograma anterior (con
    /// controles táctiles ocupan más y pueden partirse en dos líneas).
    footer_h: f32,
}

const ROW_H: f32 = 34.0;

impl App {
    /// Nombre de una capa: el que le puso el usuario o uno automático.
    pub(crate) fn layer_name(&self, l: &Layer) -> String {
        if let Some(n) = l.name.as_ref().filter(|n| !n.trim().is_empty()) {
            return n.clone();
        }
        let stem = |p: &std::path::Path| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        match &l.kind {
            Kind::Image { path } => stem(path),
            Kind::Video { path, .. } => stem(path),
            Kind::Text { text, .. } => {
                let first = text.lines().next().unwrap_or("").trim();
                let short: String = first.chars().take(28).collect();
                if short.is_empty() { "Texto".into() } else if first.chars().count() > 28 { format!("{short}…") } else { short }
            }
            Kind::Emoji { emoji, .. } => {
                let name = vector::emojis().iter().find(|e| e.emoji == emoji.as_str()).map(|e| e.name).unwrap_or("emoji");
                let mut c = name.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            }
            Kind::Shape { shape, .. } => shape.label().into(),
            Kind::Frame { frame, .. } => format!("Marco {}", frame.label().to_lowercase()),
            Kind::Graphic { name, .. } => name.clone(),
        }
    }

    /// Textura, recorte y proporción para la miniatura de una capa.
    fn layer_thumb(&self, l: &Layer) -> Option<(TextureId, Rect, f32)> {
        let c = l.crop;
        let crop_uv = Rect::from_min_max(Pos2::new(c.u0, c.v0), Pos2::new(c.u1, c.v1));
        let full = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        let aspect = l.w / l.h.max(1.0);
        match &l.kind {
            Kind::Image { path } => self.images.get(path).map(|i| (i.tex.id(), crop_uv, aspect)),
            Kind::Video { path, .. } => self
                .video_tex
                .get(&l.id)
                .map(|t| t.id())
                .or_else(|| self.videos.get(path).map(|v| v.poster.id()))
                .map(|t| (t, crop_uv, aspect)),
            Kind::Text { .. } => None,
            _ => self.vector_tex.get(&l.id).map(|v| (v.tex.id(), full, aspect)),
        }
    }

    pub(crate) fn layers_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Capas").strong());
            ui.weak(format!("{}", self.doc.layers.len()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.checkbox(&mut self.layers_ui.only_slide, "Solo este slide")
                    .on_hover_text("Muestra solo las capas del slide en el que estás trabajando");
            });
        });
        ui.separator();

        // De arriba (frente) a abajo (fondo): al revés que el orden del documento.
        let slide = self.current_slide();
        let order: Vec<usize> = (0..self.doc.layers.len())
            .rev()
            .filter(|&i| !self.layers_ui.only_slide || self.doc.slide_of(&self.doc.layers[i]) == slide)
            .collect();

        // Si la lista no deja sitio exacto a los botones, el panel crece solo.
        let footer_h = self.layers_ui.footer_h.max(30.0);
        let list_h = (ui.available_height() - footer_h).max(ROW_H);
        let mut rows: Vec<(usize, Rect)> = Vec::new();
        let mut select = None;
        let mut toggle_hidden = None;
        let mut toggle_locked = None;
        let mut start_rename = None;
        egui::ScrollArea::vertical().max_height(list_h).auto_shrink([false, false]).show(ui, |ui| {
            if order.is_empty() {
                ui.add_space(8.0);
                ui.weak(if self.layers_ui.only_slide { "Este slide no tiene capas." } else { "Aún no hay capas: añade fotos, textos o elementos de la biblioteca." });
            }
            for &idx in &order {
                let l = &self.doc.layers[idx];
                let id = l.id;
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click_and_drag());
                rows.push((idx, rect));
                let selected = self.selected == Some(id);
                let painter = ui.painter_at(rect);
                let bg = if selected {
                    ACCENT.gamma_multiply(0.35)
                } else if resp.hovered() {
                    Color32::from_gray(52)
                } else {
                    Color32::TRANSPARENT
                };
                painter.rect_filled(rect.shrink2(Vec2::new(0.0, 1.0)), 4.0, bg);
                let dim = if l.hidden { 0.4 } else { 1.0 };

                // Ojo (ocultar) y candado (bloquear).
                let eye = Rect::from_min_size(rect.min + Vec2::new(2.0, 5.0), Vec2::splat(24.0));
                let lock = Rect::from_min_size(rect.min + Vec2::new(26.0, 5.0), Vec2::splat(24.0));
                let eye_resp = ui.interact(eye, ui.scope_id().with(("eye", id)), Sense::click()).on_hover_text(if l.hidden { "Mostrar" } else { "Ocultar" });
                let lock_resp = ui.interact(lock, ui.scope_id().with(("lock", id)), Sense::click()).on_hover_text(if l.locked { "Desbloquear" } else { "Bloquear" });
                painter.text(eye.center(), Align2::CENTER_CENTER, if l.hidden { "○" } else { "●" }, FontId::proportional(13.0),
                    if eye_resp.hovered() { Color32::WHITE } else { Color32::from_gray(if l.hidden { 110 } else { 200 }) });
                painter.text(lock.center(), Align2::CENTER_CENTER, if l.locked { "🔒" } else { "🔓" }, FontId::proportional(12.0),
                    Color32::from_gray(if l.locked { 230 } else if lock_resp.hovered() { 200 } else { 90 }));
                if eye_resp.clicked() {
                    toggle_hidden = Some(idx);
                }
                if lock_resp.clicked() {
                    toggle_locked = Some(idx);
                }

                // Miniatura.
                let thumb_box = Rect::from_min_size(rect.min + Vec2::new(54.0, 3.0), Vec2::splat(28.0));
                painter.rect_filled(thumb_box, 3.0, Color32::from_gray(70));
                match self.layer_thumb(l) {
                    Some((tex, uv, aspect)) => {
                        let size = if aspect >= 1.0 { Vec2::new(26.0, 26.0 / aspect) } else { Vec2::new(26.0 * aspect, 26.0) };
                        painter.image(tex, Rect::from_center_size(thumb_box.center(), size), uv, Color32::WHITE.gamma_multiply(dim));
                    }
                    None => {
                        painter.text(thumb_box.center(), Align2::CENTER_CENTER, "T", FontId::proportional(17.0), Color32::from_gray(230));
                    }
                }

                // Nombre (o campo para renombrar) y slide.
                let name_rect = Rect::from_min_max(rect.min + Vec2::new(88.0, 4.0), Pos2::new(rect.max.x - 34.0, rect.max.y - 4.0));
                if let Some((_, buf)) = self.layers_ui.renaming.as_mut().filter(|(rid, _)| *rid == id) {
                    let edit = ui.put(name_rect, egui::TextEdit::singleline(buf).desired_width(name_rect.width()));
                    if self.layers_ui.focus_rename {
                        edit.request_focus();
                        self.layers_ui.focus_rename = false;
                    }
                    if edit.lost_focus() {
                        let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
                        let (rid, text) = self.layers_ui.renaming.take().unwrap();
                        if !cancel {
                            start_rename = Some((rid, Some(text)));
                        }
                    }
                } else {
                    let name = self.layer_name(l);
                    let color = Color32::from_gray(if selected { 255 } else { 215 }).gamma_multiply(dim);
                    let galley = ui.painter().layout(name, FontId::proportional(13.0), color, name_rect.width());
                    let text_pos = Pos2::new(name_rect.min.x, rect.center().y - galley.size().y.min(16.0) / 2.0);
                    painter.with_clip_rect(name_rect).galley(text_pos, galley, color);
                }
                painter.text(Pos2::new(rect.max.x - 8.0, rect.center().y), Align2::RIGHT_CENTER, format!("{}", self.doc.slide_of(l) + 1),
                    FontId::proportional(11.0), Color32::from_gray(120));

                if resp.double_clicked() {
                    self.layers_ui.renaming = Some((id, self.layer_name(l)));
                    self.layers_ui.focus_rename = true;
                } else if resp.clicked() || resp.drag_started() {
                    select = Some(id);
                }
                if resp.drag_started() {
                    self.layers_ui.dragging = Some(id);
                }
                resp.on_hover_text("Clic: seleccionar · Doble clic: renombrar · Arrastrar: cambiar de orden");
            }

            // Arrastrar para reordenar: línea donde caerá la capa.
            if let Some(drag_id) = self.layers_ui.dragging {
                let pointer = ui.input(|i| i.pointer.interact_pos());
                let target = pointer.and_then(|p| drop_target(&rows, p));
                if let Some((row, above)) = target {
                    let r = rows[row].1;
                    let y = if above { r.min.y } else { r.max.y };
                    ui.painter().line_segment([Pos2::new(r.min.x, y), Pos2::new(r.max.x, y)], Stroke::new(2.0, ACCENT));
                }
                if ui.input(|i| i.pointer.any_released()) {
                    self.layers_ui.dragging = None;
                    if let Some((row, above)) = target {
                        self.move_layer_to(drag_id, rows[row].0, above);
                    }
                }
            }
        });

        if let Some(id) = select {
            self.selected = Some(id);
        }
        if let Some(idx) = toggle_hidden {
            self.checkpoint();
            let l = &mut self.doc.layers[idx];
            l.hidden = !l.hidden;
        }
        if let Some(idx) = toggle_locked {
            self.checkpoint();
            let l = &mut self.doc.layers[idx];
            l.locked = !l.locked;
        }
        if let Some((id, Some(text))) = start_rename {
            self.checkpoint();
            if let Some(l) = self.doc.layer_mut(id) {
                let t = text.trim().to_string();
                l.name = (!t.is_empty()).then_some(t);
            }
        }

        // Botones inferiores, como en Photoshop.
        let footer_top = ui.cursor().min.y;
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let has = self.selected.and_then(|id| self.doc.index_of(id));
            let enabled = has.is_some();
            if ui.add_enabled(enabled, egui::Button::new("⬆")).on_hover_text("Subir una capa").clicked() {
                self.reorder(1, false);
            }
            if ui.add_enabled(enabled, egui::Button::new("⬇")).on_hover_text("Bajar una capa").clicked() {
                self.reorder(-1, false);
            }
            if ui.add_enabled(enabled, egui::Button::new("Duplicar")).on_hover_text("⌘D").clicked() {
                self.duplicate_selected();
            }
            let (hidden, locked) = has.map(|i| (self.doc.layers[i].hidden, self.doc.layers[i].locked)).unwrap_or((false, false));
            if ui.add_enabled(enabled, egui::Button::new(if hidden { "Mostrar" } else { "Ocultar" })).clicked() {
                if let Some(i) = has {
                    self.checkpoint();
                    self.doc.layers[i].hidden = !hidden;
                }
            }
            if ui.add_enabled(enabled, egui::Button::new(if locked { "Desbloquear" } else { "Bloquear" })).clicked() {
                if let Some(i) = has {
                    self.checkpoint();
                    self.doc.layers[i].locked = !locked;
                }
            }
            if ui.add_enabled(enabled, egui::Button::new(egui::RichText::new("🗑").color(Color32::from_rgb(255, 120, 120))))
                .on_hover_text("Eliminar (⌫)")
                .clicked()
            {
                self.delete_selected();
            }
        });
        self.layers_ui.footer_h = ui.cursor().min.y - footer_top;
    }

    /// Mueve una capa justo encima (`above`) o debajo de la capa en `target_idx`
    /// tal como se ven en el panel (arriba = más al frente).
    pub(crate) fn move_layer_to(&mut self, id: u64, target_idx: usize, above: bool) {
        let Some(from) = self.doc.index_of(id) else { return };
        // En el documento, «encima» significa un índice mayor.
        let mut to = if above { target_idx + 1 } else { target_idx };
        if from < to {
            to -= 1;
        }
        if to == from {
            return;
        }
        self.checkpoint();
        let l = self.doc.layers.remove(from);
        self.doc.layers.insert(to.min(self.doc.layers.len()), l);
    }
}

/// Fila bajo el puntero y si cae en su mitad superior.
fn drop_target(rows: &[(usize, Rect)], p: Pos2) -> Option<(usize, bool)> {
    if rows.is_empty() {
        return None;
    }
    if let Some(i) = rows.iter().position(|(_, r)| p.y >= r.min.y && p.y < r.max.y) {
        return Some((i, p.y < rows[i].1.center().y));
    }
    // Por encima de la primera o por debajo de la última fila.
    if p.y < rows[0].1.min.y {
        Some((0, true))
    } else {
        Some((rows.len() - 1, false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soltar_entre_filas() {
        let row = |y: f32| Rect::from_min_size(Pos2::new(0.0, y), Vec2::new(100.0, ROW_H));
        let rows = vec![(2, row(0.0)), (1, row(ROW_H)), (0, row(2.0 * ROW_H))];
        assert_eq!(drop_target(&rows, Pos2::new(10.0, 5.0)), Some((0, true)));
        assert_eq!(drop_target(&rows, Pos2::new(10.0, ROW_H + 30.0)), Some((1, false)));
        assert_eq!(drop_target(&rows, Pos2::new(10.0, 500.0)), Some((2, false)));
        assert_eq!(drop_target(&rows, Pos2::new(10.0, -20.0)), Some((0, true)));
    }
}
