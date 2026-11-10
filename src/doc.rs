//! Modelo del documento: un lienzo panorámico dividido en slides.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Altura de línea del texto, relativa al tamaño de fuente.
pub const LINE_H: f32 = 1.2;

/// Máximo de slides que Instagram publica en un solo post. El lienzo no
/// tiene límite; pasado este número solo se avisa.
pub const INSTAGRAM_MAX: u32 = 20;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl TextAlign {
    /// Desplazamiento horizontal de una línea de ancho `line_w` dentro de una caja de ancho `box_w`.
    pub fn offset(self, box_w: f32, line_w: f32) -> f32 {
        match self {
            TextAlign::Left => 0.0,
            TextAlign::Center => (box_w - line_w) / 2.0,
            TextAlign::Right => box_w - line_w,
        }
    }
}

fn one() -> f32 {
    1.0
}

/// Parte visible de la foto original, en fracciones de 0 a 1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

impl Default for Crop {
    fn default() -> Self {
        Self::FULL
    }
}

impl Crop {
    pub const FULL: Crop = Crop { u0: 0.0, v0: 0.0, u1: 1.0, v1: 1.0 };

    pub fn is_full(&self) -> bool {
        *self == Self::FULL
    }

    /// Recorte centrado de una foto de proporción `aspect` para que
    /// llene un rectángulo de proporción `target`.
    pub fn cover(aspect: f32, target: f32) -> Crop {
        if aspect > target {
            let f = target / aspect;
            Crop { u0: (1.0 - f) / 2.0, u1: (1.0 + f) / 2.0, ..Crop::FULL }
        } else {
            let f = aspect / target;
            Crop { v0: (1.0 - f) / 2.0, v1: (1.0 + f) / 2.0, ..Crop::FULL }
        }
    }
}

/// Lado de una foto que se arrastra para recortarla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// Tamaño mínimo de un recorte, en píxeles del lienzo.
pub const MIN_CROP: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    Square,    // 1:1
    Portrait,  // 4:5
    Classic,   // 3:4
    Story,     // 9:16
}

impl Format {
    pub const ALL: [Format; 4] = [Format::Portrait, Format::Square, Format::Classic, Format::Story];

    pub fn size(self) -> (u32, u32) {
        match self {
            Format::Square => (1080, 1080),
            Format::Portrait => (1080, 1350),
            Format::Classic => (1080, 1440),
            Format::Story => (1080, 1920),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Square => "Cuadrado 1:1",
            Format::Portrait => "Vertical 4:5",
            Format::Classic => "Vertical 3:4",
            Format::Story => "Historia 9:16",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Kind {
    Image { path: PathBuf },
    Text {
        text: String,
        size: f32,
        color: [u8; 4],
        #[serde(default)]
        align: TextAlign,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    pub id: u64,
    pub kind: Kind,
    /// Posición y tamaño en píxeles del lienzo (no de pantalla).
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// 0 = invisible, 1 = opaca.
    #[serde(default = "one")]
    pub opacity: f32,
    /// Radio de las esquinas en píxeles del lienzo (solo fotos).
    #[serde(default)]
    pub radius: f32,
    /// Qué parte de la foto se ve dentro del marco (x, y, w, h).
    #[serde(default)]
    pub crop: Crop,
}

impl Layer {
    pub fn new(id: u64, kind: Kind, (x, y, w, h): (f32, f32, f32, f32)) -> Self {
        Self { id, kind, x, y, w, h, opacity: 1.0, radius: 0.0, crop: Crop::FULL }
    }

    /// Rectángulo que ocuparía la foto entera sin recortar, en el lienzo.
    pub fn full_rect(&self) -> (f32, f32, f32, f32) {
        let c = self.crop;
        let fw = self.w / (c.u1 - c.u0).max(1e-6);
        let fh = self.h / (c.v1 - c.v0).max(1e-6);
        (self.x - c.u0 * fw, self.y - c.v0 * fh, fw, fh)
    }

    /// Llena exactamente `rect` con la foto, recortando lo que sobre por igual
    /// en ambos lados. `aspect` es la proporción de la foto original.
    pub fn set_cover(&mut self, aspect: f32, rect: (f32, f32, f32, f32)) {
        (self.x, self.y, self.w, self.h) = rect;
        self.crop = Crop::cover(aspect, rect.2 / rect.3.max(1e-6));
    }

    /// Mueve un lado del marco hasta `to` (coordenada del lienzo) sin mover
    /// ni deformar la foto: lo que queda fuera se recorta. No puede ir más
    /// allá del borde de la foto original.
    pub fn crop_edge(&mut self, edge: Edge, to: f32) {
        let (fx, fy, fw, fh) = self.full_rect();
        let (right, bottom) = (self.x + self.w, self.y + self.h);
        match edge {
            Edge::Left => {
                let x = to.clamp(fx, right - MIN_CROP.min(self.w));
                self.crop.u0 = (x - fx) / fw;
                self.x = x;
                self.w = right - x;
            }
            Edge::Right => {
                let r = to.clamp(self.x + MIN_CROP.min(self.w), fx + fw);
                self.crop.u1 = (r - fx) / fw;
                self.w = r - self.x;
            }
            Edge::Top => {
                let y = to.clamp(fy, bottom - MIN_CROP.min(self.h));
                self.crop.v0 = (y - fy) / fh;
                self.y = y;
                self.h = bottom - y;
            }
            Edge::Bottom => {
                let b = to.clamp(self.y + MIN_CROP.min(self.h), fy + fh);
                self.crop.v1 = (b - fy) / fh;
                self.h = b - self.y;
            }
        }
    }

    /// Desplaza la foto dentro del marco (dx, dy en píxeles del lienzo),
    /// sin dejar huecos: el recorte nunca sale de la foto original.
    pub fn pan_crop(&mut self, dx: f32, dy: f32) {
        let (_, _, fw, fh) = self.full_rect();
        let c = &mut self.crop;
        let (cw, ch) = (c.u1 - c.u0, c.v1 - c.v0);
        let u0 = (c.u0 - dx / fw).clamp(0.0, 1.0 - cw);
        let v0 = (c.v0 - dy / fh).clamp(0.0, 1.0 - ch);
        *c = Crop { u0, v0, u1: u0 + cw, v1: v0 + ch };
    }

    /// Quita el recorte: el marco vuelve a mostrar la foto entera.
    pub fn reset_crop(&mut self) {
        (self.x, self.y, self.w, self.h) = self.full_rect();
        self.crop = Crop::FULL;
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
    pub fn is_image(&self) -> bool {
        matches!(self.kind, Kind::Image { .. })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Doc {
    pub version: u32,
    pub format: Format,
    pub slides: u32,
    pub background: [u8; 4],
    #[serde(default)]
    pub font: Option<PathBuf>,
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub next_id: u64,
}

impl Default for Doc {
    fn default() -> Self {
        Self {
            version: 1,
            format: Format::Portrait,
            slides: 3,
            background: [255, 255, 255, 255],
            font: None,
            layers: Vec::new(),
            next_id: 1,
        }
    }
}

impl Doc {
    pub fn slide_size(&self) -> (f32, f32) {
        let (w, h) = self.format.size();
        (w as f32, h as f32)
    }

    pub fn canvas_size(&self) -> (f32, f32) {
        let (w, h) = self.slide_size();
        (w * self.slides as f32, h)
    }

    pub fn new_id(&mut self) -> u64 {
        self.next_id = self.next_id.max(self.layers.iter().map(|l| l.id).max().unwrap_or(0) + 1);
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    pub fn layer_mut(&mut self, id: u64) -> Option<&mut Layer> {
        self.layers.iter_mut().find(|l| l.id == id)
    }

    /// Índice del slide donde cae el centro de la capa.
    pub fn slide_of(&self, l: &Layer) -> u32 {
        let (sw, _) = self.slide_size();
        let cx = l.x + l.w / 2.0;
        ((cx / sw).floor().max(0.0) as u32).min(self.slides - 1)
    }

    /// Rectángulo (x, y, w, h) que ocupan `span` slides a partir de `start`.
    pub fn slides_rect(&self, start: u32, span: u32) -> (f32, f32, f32, f32) {
        let (sw, sh) = self.slide_size();
        let start = start.min(self.slides - 1);
        let span = span.max(1).min(self.slides - start);
        (start as f32 * sw, 0.0, span as f32 * sw, sh)
    }

    /// Inserta un slide vacío en la posición `at` (0 = al principio) y
    /// desplaza a la derecha todo lo que tenga su centro a partir de ahí.
    pub fn insert_slide(&mut self, at: u32) {
        let (sw, _) = self.slide_size();
        let at = at.min(self.slides);
        let edge = at as f32 * sw;
        for l in &mut self.layers {
            if l.x + l.w / 2.0 >= edge {
                l.x += sw;
            }
        }
        self.slides += 1;
    }

    /// Elimina el slide `at` junto con las capas centradas en él, y
    /// desplaza a la izquierda las que estén más allá.
    pub fn remove_slide(&mut self, at: u32) -> bool {
        if self.slides <= 1 || at >= self.slides {
            return false;
        }
        let (sw, _) = self.slide_size();
        let slide_of: Vec<u32> = self.layers.iter().map(|l| self.slide_of(l)).collect();
        self.layers = std::mem::take(&mut self.layers)
            .into_iter()
            .zip(slide_of)
            .filter(|&(_, s)| s != at)
            .map(|(mut l, s)| {
                if s > at {
                    l.x -= sw;
                }
                l
            })
            .collect();
        self.slides -= 1;
        true
    }
}

/// Coloca una imagen de proporción `aspect` (w/h) dentro del rectángulo.
/// `cover = true` la recorta llenándolo; `false` la encaja completa.
pub fn fit(aspect: f32, rect: (f32, f32, f32, f32), cover: bool) -> (f32, f32, f32, f32) {
    let (rx, ry, rw, rh) = rect;
    let rect_aspect = rw / rh;
    let wider = aspect > rect_aspect;
    let (w, h) = if wider == cover {
        (rh * aspect, rh)
    } else {
        (rw, rw / aspect)
    };
    (rx + (rw - w) / 2.0, ry + (rh - h) / 2.0, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with(xs: &[f32]) -> Doc {
        let mut d = Doc { slides: 3, ..Default::default() };
        for (i, &x) in xs.iter().enumerate() {
            d.layers.push(Layer::new(i as u64 + 1, Kind::Image { path: "a".into() }, (x, 0.0, 100.0, 100.0)));
        }
        d
    }

    #[test]
    fn insertar_slide_desplaza_lo_que_sigue() {
        // Una capa en cada slide (ancho de slide 1080).
        let mut d = doc_with(&[100.0, 1180.0, 2260.0]);
        d.insert_slide(1);
        assert_eq!(d.slides, 4);
        let xs: Vec<f32> = d.layers.iter().map(|l| l.x).collect();
        assert_eq!(xs, [100.0, 2260.0, 3340.0]);
    }

    #[test]
    fn eliminar_slide_borra_su_contenido_y_junta_el_resto() {
        let mut d = doc_with(&[100.0, 1180.0, 2260.0]);
        assert!(d.remove_slide(1));
        assert_eq!(d.slides, 2);
        let xs: Vec<f32> = d.layers.iter().map(|l| l.x).collect();
        assert_eq!(xs, [100.0, 1180.0]);
        assert!(d.remove_slide(1));
        assert!(!d.remove_slide(0), "siempre queda al menos un slide");
    }

    fn photo(rect: (f32, f32, f32, f32)) -> Layer {
        Layer::new(1, Kind::Image { path: "a".into() }, rect)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn recortar_por_la_izquierda_no_mueve_la_foto() {
        let mut l = photo((100.0, 0.0, 1000.0, 500.0));
        l.crop_edge(Edge::Left, 300.0);
        assert!(close(l.x, 300.0) && close(l.w, 800.0));
        assert!(close(l.crop.u0, 0.2));
        // La foto completa sigue en el mismo sitio.
        let (fx, _, fw, _) = l.full_rect();
        assert!(close(fx, 100.0) && close(fw, 1000.0));
        // Se puede volver a extender, pero no más allá del borde original.
        l.crop_edge(Edge::Left, -500.0);
        assert!(close(l.x, 100.0) && close(l.crop.u0, 0.0));
    }

    #[test]
    fn recortar_por_la_derecha_y_abajo() {
        let mut l = photo((0.0, 0.0, 1000.0, 500.0));
        l.crop_edge(Edge::Right, 600.0);
        l.crop_edge(Edge::Bottom, 250.0);
        assert!(close(l.w, 600.0) && close(l.h, 250.0));
        assert!(close(l.crop.u1, 0.6) && close(l.crop.v1, 0.5));
        // Nunca se cruza con el lado opuesto.
        l.crop_edge(Edge::Right, -100.0);
        assert!(close(l.w, MIN_CROP));
    }

    #[test]
    fn mover_la_foto_dentro_del_marco() {
        let mut l = photo((0.0, 0.0, 1000.0, 1000.0));
        l.crop_edge(Edge::Right, 500.0);
        // Arrastrar a la izquierda enseña la parte derecha de la foto.
        l.pan_crop(-200.0, 0.0);
        assert!(close(l.crop.u0, 0.2) && close(l.crop.u1, 0.7));
        // Sin pasarse del borde.
        l.pan_crop(-5000.0, 0.0);
        assert!(close(l.crop.u0, 0.5) && close(l.crop.u1, 1.0));
        assert!(close(l.x, 0.0) && close(l.w, 500.0), "el marco no se mueve");
        l.reset_crop();
        assert!(l.crop.is_full());
        assert!(close(l.x, -500.0) && close(l.w, 1000.0));
    }

    #[test]
    fn rellenar_recorta_centrado() {
        let mut l = photo((0.0, 0.0, 10.0, 10.0));
        // Foto 2:1 en un slide 4:5 → se ve el centro de la foto.
        l.set_cover(2.0, (1080.0, 0.0, 1080.0, 1350.0));
        assert!(close(l.x, 1080.0) && close(l.w, 1080.0) && close(l.h, 1350.0));
        assert!(close(l.crop.u0 + l.crop.u1, 1.0));
        assert!(close((l.crop.u1 - l.crop.u0) * 2.0, 0.8));
    }

    #[test]
    fn slides_sin_limite() {
        let mut d = doc_with(&[100.0]);
        for _ in 0..(INSTAGRAM_MAX * 10) {
            d.insert_slide(0);
        }
        assert_eq!(d.slides, 3 + INSTAGRAM_MAX * 10);
        // La capa sigue en el que era su slide, ahora al final.
        assert_eq!(d.slide_of(&d.layers[0]), INSTAGRAM_MAX * 10);
    }

    #[test]
    fn proyectos_antiguos_siguen_abriendo() {
        let json = r#"{"version":1,"format":"Portrait","slides":2,"background":[255,255,255,255],
            "layers":[{"id":1,"kind":{"Text":{"text":"Hola","size":96.0,"color":[0,0,0,255]}},"x":0,"y":0,"w":10,"h":10}]}"#;
        let d: Doc = serde_json::from_str(json).unwrap();
        assert_eq!(d.layers[0].opacity, 1.0);
        assert_eq!(d.layers[0].radius, 0.0);
        assert!(d.layers[0].crop.is_full());
        assert!(matches!(d.layers[0].kind, Kind::Text { align: TextAlign::Left, .. }));
    }
}
