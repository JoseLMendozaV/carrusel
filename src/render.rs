//! Carga de imágenes, dibujo de capas y exportación (imágenes y videos MP4).

use crate::doc::{Doc, Kind, Layer, LINE_H};
use crate::{vector, video};
use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use image::{imageops, DynamicImage, ImageDecoder, ImageEncoder, ImageReader, Rgba, RgbaImage};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Abre una imagen respetando la orientación EXIF (fotos de iPhone, etc.).
pub fn load_image(path: &Path) -> Result<RgbaImage, String> {
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", path.display());
    let reader = ImageReader::open(path)
        .map_err(|e| err(&e))?
        .with_guessed_format()
        .map_err(|e| err(&e))?;
    let mut decoder = reader.into_decoder().map_err(|e| err(&e))?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| err(&e))?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img.to_rgba8())
}

/// Decodifica una imagen ya leída en memoria, respetando la orientación EXIF.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn decode_image(bytes: &[u8]) -> Result<RgbaImage, String> {
    let reader = ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Ok(img.to_rgba8())
}

/// Codifica una imagen en memoria (JPG calidad 95 o PNG), para la versión web.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn encode_image(img: RgbaImage, format: ExportFormat) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    match format {
        ExportFormat::Png => image::codecs::png::PngEncoder::new(&mut out)
            .write_image(img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgba8)
            .map_err(|e| e.to_string())?,
        ExportFormat::Jpg => {
            let rgb = DynamicImage::ImageRgba8(img).to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95).encode_image(&rgb).map_err(|e| e.to_string())?
        }
    }
    Ok(out)
}

/// Ancho de una sola línea de texto en píxeles del lienzo.
pub fn line_width(font: &FontArc, line: &str, size: f32) -> f32 {
    let sf = font.as_scaled(PxScale::from(size));
    let mut w = 0.0;
    let mut prev = None;
    for c in line.chars() {
        let g = sf.glyph_id(c);
        if let Some(p) = prev {
            w += sf.kern(p, g);
        }
        w += sf.h_advance(g);
        prev = Some(g);
    }
    w
}

/// Ancho y alto de un texto (multilínea) en píxeles del lienzo.
pub fn measure_text(font: &FontArc, text: &str, size: f32) -> (f32, f32) {
    let max_w = text.split('\n').map(|l| line_width(font, l, size)).fold(0.0, f32::max);
    let lines = text.split('\n').count();
    (max_w.max(size * 0.3), size * LINE_H * lines.max(1) as f32)
}

fn blend(dst: &mut Rgba<u8>, color: [u8; 4], coverage: f32) {
    let a = (coverage.clamp(0.0, 1.0)) * color[3] as f32 / 255.0;
    if a <= 0.0 {
        return;
    }
    let da = dst[3] as f32 / 255.0;
    let out_a = a + da * (1.0 - a);
    for i in 0..3 {
        let s = color[i] as f32;
        let d = dst[i] as f32;
        dst[i] = ((s * a + d * da * (1.0 - a)) / out_a.max(1e-6)).round() as u8;
    }
    dst[3] = (out_a * 255.0).round() as u8;
}

/// Un texto de una línea dibujado sobre fondo transparente (muestras de fuentes).
pub fn text_image(font: &FontArc, text: &str, size: f32, color: [u8; 4]) -> RgbaImage {
    let (w, h) = measure_text(font, text, size);
    let mut img = RgbaImage::new(w.ceil() as u32 + 2, h.ceil() as u32 + 2);
    let kind = Kind::Text { text: text.into(), size, color, align: crate::doc::TextAlign::Left, background: None, shadow: false };
    draw_text(&mut img, font, &Layer::new(0, kind, (1.0, 1.0, w, h)));
    img
}

/// Relleno alrededor del texto cuando lleva recuadro de fondo.
pub fn text_padding(size: f32) -> (f32, f32) {
    (size * 0.3, size * 0.15)
}

fn draw_glyphs(canvas: &mut RgbaImage, font: &FontArc, l: &Layer, (dx, dy): (f32, f32), color: [u8; 4]) {
    let Kind::Text { text, size, align, .. } = &l.kind else { return };
    let size = *size;
    let scale = PxScale::from(size);
    let sf = font.as_scaled(scale);
    let (cw, ch) = (canvas.width() as i32, canvas.height() as i32);
    for (i, line) in text.split('\n').enumerate() {
        let x = l.x + dx + align.offset(l.w, line_width(font, line, size));
        let mut caret = point(x, l.y + dy + i as f32 * size * LINE_H + sf.ascent());
        let mut prev = None;
        for c in line.chars() {
            let gid = sf.glyph_id(c);
            if let Some(p) = prev {
                caret.x += sf.kern(p, gid);
            }
            let glyph = gid.with_scale_and_position(scale, caret);
            caret.x += sf.h_advance(gid);
            prev = Some(gid);
            if let Some(og) = font.outline_glyph(glyph) {
                let b = og.px_bounds();
                og.draw(|gx, gy, cov| {
                    let px = b.min.x as i32 + gx as i32;
                    let py = b.min.y as i32 + gy as i32;
                    if px >= 0 && py >= 0 && px < cw && py < ch {
                        blend(canvas.get_pixel_mut(px as u32, py as u32), color, cov);
                    }
                });
            }
        }
    }
}

fn with_opacity(mut c: [u8; 4], opacity: f32) -> [u8; 4] {
    c[3] = (c[3] as f32 * opacity.clamp(0.0, 1.0)).round() as u8;
    c
}

fn draw_text(canvas: &mut RgbaImage, font: &FontArc, l: &Layer) {
    let Kind::Text { size, color, background, shadow, .. } = &l.kind else { return };
    if let Some(bg) = background {
        let (px, py) = text_padding(*size);
        let rect = (l.x - px, l.y - py, l.w + 2.0 * px, l.h + 2.0 * py);
        fill_rrect(canvas, rect, size * 0.35, with_opacity(*bg, l.opacity));
    }
    if *shadow {
        let off = (size * 0.05).max(1.0);
        draw_glyphs(canvas, font, l, (off, off), with_opacity([0, 0, 0, 115], l.opacity));
    }
    draw_glyphs(canvas, font, l, (0.0, 0.0), with_opacity(*color, l.opacity));
}

/// Cobertura (0 a 1) de un rectángulo redondeado en el punto (px, py),
/// con medio píxel de suavizado en el borde.
fn rrect_coverage(px: f32, py: f32, (x, y, w, h): (f32, f32, f32, f32), radius: f32) -> f32 {
    if w <= 0.0 || h <= 0.0 {
        return 0.0;
    }
    let r = radius.clamp(0.0, w.min(h) / 2.0);
    let (hx, hy) = (w / 2.0, h / 2.0);
    let qx = (px - (x + hx)).abs() - (hx - r);
    let qy = (py - (y + hy)).abs() - (hy - r);
    let dist = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
    (0.5 - dist).clamp(0.0, 1.0)
}

/// Recorre los píxeles del lienzo dentro de `rect` y mezcla `color` con la
/// cobertura que devuelva `coverage(px, py)`.
fn paint_area(canvas: &mut RgbaImage, rect: (f32, f32, f32, f32), color: [u8; 4], coverage: impl Fn(f32, f32) -> f32) {
    let (x, y, w, h) = rect;
    let x0 = x.floor().max(0.0) as u32;
    let y0 = y.floor().max(0.0) as u32;
    let x1 = ((x + w).ceil().max(0.0) as u32).min(canvas.width());
    let y1 = ((y + h).ceil().max(0.0) as u32).min(canvas.height());
    for py in y0..y1 {
        for px in x0..x1 {
            let cov = coverage(px as f32 + 0.5, py as f32 + 0.5);
            if cov > 0.0 {
                blend(canvas.get_pixel_mut(px, py), color, cov);
            }
        }
    }
}

fn fill_rrect(canvas: &mut RgbaImage, rect: (f32, f32, f32, f32), radius: f32, color: [u8; 4]) {
    paint_area(canvas, rect, color, |px, py| rrect_coverage(px, py, rect, radius));
}

/// Cuánto cubre el rectángulo redondeado de la capa el punto (px, py).
fn rounded_coverage(px: f32, py: f32, l: &Layer) -> f32 {
    rrect_coverage(px, py, (l.x, l.y, l.w, l.h), l.radius)
}

/// Marco de color alrededor de una foto o video, por dentro de su borde.
fn draw_border(canvas: &mut RgbaImage, l: &Layer) {
    let t = l.border.min(l.w.min(l.h) / 2.0);
    if t <= 0.0 {
        return;
    }
    let outer = (l.x, l.y, l.w, l.h);
    let inner = (l.x + t, l.y + t, l.w - 2.0 * t, l.h - 2.0 * t);
    let (r, ri) = (l.radius, (l.radius - t).max(0.0));
    paint_area(canvas, outer, with_opacity(l.border_color, l.opacity), |px, py| {
        (rrect_coverage(px, py, outer, r) - rrect_coverage(px, py, inner, ri)).max(0.0)
    });
}

/// Dibuja una foto escalando solo la parte que cae dentro del lienzo, para que
/// una capa enorme (muy ampliada o casi fuera) no dispare la memoria.
fn draw_image(canvas: &mut RgbaImage, src: &RgbaImage, l: &Layer) {
    if l.w < 0.5 || l.h < 0.5 || l.opacity <= 0.0 || src.width() == 0 || src.height() == 0 {
        return;
    }
    let (cw, ch) = (canvas.width() as f32, canvas.height() as f32);
    // Parte visible de la capa, en píxeles enteros del lienzo.
    let (vx0, vy0) = (l.x.max(0.0).floor(), l.y.max(0.0).floor());
    let (vx1, vy1) = ((l.x + l.w).min(cw).ceil(), (l.y + l.h).min(ch).ceil());
    if vx1 <= vx0 || vy1 <= vy0 {
        return;
    }
    // La región equivalente en la foto original, dentro del recorte de la capa.
    let c = l.crop;
    let (ox, oy) = (c.u0 * src.width() as f32, c.v0 * src.height() as f32);
    let (ex, ey) = (c.u1 * src.width() as f32, c.v1 * src.height() as f32);
    let (sx, sy) = ((ex - ox) / l.w, (ey - oy) / l.h);
    let u0 = (ox + (vx0 - l.x) * sx).floor().max(0.0);
    let v0 = (oy + (vy0 - l.y) * sy).floor().max(0.0);
    let u1 = (ox + (vx1 - l.x) * sx).ceil().min(ex.ceil()).min(src.width() as f32);
    let v1 = (oy + (vy1 - l.y) * sy).ceil().min(ey.ceil()).min(src.height() as f32);
    if u1 <= u0 || v1 <= v0 || sx <= 0.0 || sy <= 0.0 {
        return;
    }
    let crop = imageops::crop_imm(src, u0 as u32, v0 as u32, (u1 - u0) as u32, (v1 - v0) as u32).to_image();
    // Dónde cae exactamente ese recorte en el lienzo.
    let tx = (l.x + (u0 - ox) / sx).round();
    let ty = (l.y + (v0 - oy) / sy).round();
    let tw = ((u1 - u0) / sx).round().max(1.0) as u32;
    let th = ((v1 - v0) / sy).round().max(1.0) as u32;
    let mut piece = if (tw, th) == crop.dimensions() {
        crop // ya tiene el tamaño final (fotogramas de video): no hace falta escalar
    } else {
        imageops::resize(&crop, tw, th, imageops::FilterType::CatmullRom)
    };

    let opacity = l.opacity.clamp(0.0, 1.0);
    if l.radius > 0.0 || opacity < 1.0 || !l.crop.is_full() {
        for (px, py, p) in piece.enumerate_pixels_mut() {
            let cov = rounded_coverage(tx + px as f32 + 0.5, ty + py as f32 + 0.5, l) * opacity;
            p[3] = (p[3] as f32 * cov).round() as u8;
        }
    }
    imageops::overlay(canvas, &piece, tx as i64, ty as i64);
    draw_border(canvas, l);
}

/// Emojis, calcomanías, formas y marcos: se rasteriza solo la parte visible.
fn draw_vector(canvas: &mut RgbaImage, l: &Layer) {
    let Some(svg) = vector::layer_svg(&l.kind, l.w, l.h) else { return };
    let (cw, ch) = (canvas.width() as f32, canvas.height() as f32);
    let (vx0, vy0) = (l.x.max(0.0).floor(), l.y.max(0.0).floor());
    let (vx1, vy1) = ((l.x + l.w).min(cw).ceil(), (l.y + l.h).min(ch).ceil());
    if vx1 <= vx0 || vy1 <= vy0 || l.opacity <= 0.0 {
        return;
    }
    let size = ((vx1 - vx0) as u32, (vy1 - vy0) as u32);
    let Some(mut piece) = vector::rasterize(&svg, 1.0, (vx0 - l.x, vy0 - l.y), size) else { return };
    let opacity = l.opacity.clamp(0.0, 1.0);
    if opacity < 1.0 {
        for p in piece.pixels_mut() {
            p[3] = (p[3] as f32 * opacity).round() as u8;
        }
    }
    imageops::overlay(canvas, &piece, vx0 as i64, vy0 as i64);
}

/// Fotogramas de video por id de capa (para dibujar videos como imagen).
pub type Frames = HashMap<u64, RgbaImage>;

fn draw_layer(canvas: &mut RgbaImage, l: &Layer, images: &HashMap<PathBuf, Arc<RgbaImage>>, font: &FontArc, frames: &Frames) {
    match &l.kind {
        Kind::Image { path } => {
            if let Some(src) = images.get(path) {
                draw_image(canvas, src, l);
            }
        }
        Kind::Video { .. } => {
            if let Some(frame) = frames.get(&l.id) {
                draw_image(canvas, frame, l);
            }
        }
        Kind::Text { .. } => draw_text(canvas, font, l),
        Kind::Emoji { .. } | Kind::Shape { .. } | Kind::Frame { .. } | Kind::Graphic { .. } => draw_vector(canvas, l),
    }
}

/// Dibuja `layers` en la franja del lienzo que empieza en `x0` y mide `w`
/// píxeles de ancho. Sin `background` el resultado es transparente.
fn render_layers<'a>(
    doc: &Doc,
    layers: impl IntoIterator<Item = &'a Layer>,
    images: &HashMap<PathBuf, Arc<RgbaImage>>,
    font: &FontArc,
    frames: &Frames,
    x0: f32,
    w: u32,
    background: bool,
) -> RgbaImage {
    let (_, ch) = doc.canvas_size();
    let fill = if background { Rgba(doc.background) } else { Rgba([0, 0, 0, 0]) };
    let mut canvas = RgbaImage::from_pixel(w, ch as u32, fill);
    let x1 = x0 + w as f32;
    for layer in layers.into_iter().filter(|l| !l.hidden) {
        // Lo que no toca esta franja se salta (los textos pueden sobresalir
        // un poco de su caja, así que siempre se dibujan).
        let text = matches!(layer.kind, Kind::Text { .. });
        if !text && (layer.x + layer.w <= x0 || layer.x >= x1) {
            continue;
        }
        let mut l = layer.clone();
        l.x -= x0;
        draw_layer(&mut canvas, &l, images, font, frames);
    }
    canvas
}

/// La franja `[x0, x0 + w)` del lienzo con todas sus capas.
pub fn render_region(doc: &Doc, images: &HashMap<PathBuf, Arc<RgbaImage>>, font: &FontArc, x0: f32, w: u32) -> RgbaImage {
    render_layers(doc, &doc.layers, images, font, &Frames::new(), x0, w, true)
}

/// Dibuja el lienzo completo (todas las slides juntas) a resolución final.
#[cfg(test)]
pub fn render_canvas(doc: &Doc, images: &HashMap<PathBuf, Arc<RgbaImage>>, font: &FontArc) -> RgbaImage {
    render_region(doc, images, font, 0.0, doc.canvas_size().0 as u32)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Jpg,
    Png,
}

/// Ancho máximo que admite un JPG.
const JPEG_MAX: u32 = 65_535;
/// Lado máximo al que se decodifica un video para exportarlo.
const VIDEO_DECODE_MAX: f32 = 4096.0;

/// Nombre de cada archivo: slide_01, slide_02… (con más ceros si hacen falta,
/// para que se ordenen bien: slide_001 … slide_150).
pub fn slide_name(i: u32, total: u32, ext: &str) -> String {
    let digits = total.to_string().len().max(2);
    format!("slide_{:0digits$}.{ext}", i + 1)
}

fn save_image(img: RgbaImage, path: &Path, format: ExportFormat) -> Result<(), String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let err = |e: &dyn std::fmt::Display| format!("{name}: {e}");
    match format {
        ExportFormat::Png => img.save(path).map_err(|e| err(&e)),
        ExportFormat::Jpg => {
            let rgb = DynamicImage::ImageRgba8(img).to_rgb8();
            let file = std::fs::File::create(path).map_err(|e| err(&e))?;
            let mut w = std::io::BufWriter::new(file);
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, 95)
                .encode_image(&rgb)
                .map_err(|e| err(&e))
        }
    }
}

/// Capas de video que tocan la franja `[x0, x0 + w)`.
fn videos_in(doc: &Doc, x0: f32, w: f32) -> Vec<&Layer> {
    doc.layers.iter().filter(|l| l.is_video() && !l.hidden && l.x < x0 + w && l.x + l.w > x0).collect()
}

/// Tamaño al que se decodifica un video: el de la foto entera en el lienzo.
fn decode_size(l: &Layer) -> (u32, u32) {
    let (_, _, fw, fh) = l.full_rect();
    let s = (VIDEO_DECODE_MAX / fw.max(fh)).min(1.0);
    ((fw * s).round().max(2.0) as u32, (fh * s).round().max(2.0) as u32)
}

fn video_source(l: &Layer) -> (&Path, f32, bool) {
    match &l.kind {
        Kind::Video { path, start, muted } => (path.as_path(), *start, *muted),
        _ => unreachable!("solo capas de video"),
    }
}

/// Compone fotogramas de video: las capas fijas se dibujan una sola vez por
/// slide (en «tramos» entre un video y el siguiente, para respetar el orden de
/// las capas) y en cada fotograma solo se pegan los tramos y los videos.
struct Compositor<'a> {
    doc: &'a Doc,
    images: &'a HashMap<PathBuf, Arc<RgbaImage>>,
    font: &'a FontArc,
    videos: Vec<(&'a Layer, video::FrameReader)>,
    /// Tramos ya dibujados de cada slide (videos.len() + 1 por slide).
    cache: HashMap<u32, Vec<RgbaImage>>,
}

impl<'a> Compositor<'a> {
    fn new(doc: &'a Doc, images: &'a HashMap<PathBuf, Arc<RgbaImage>>, font: &'a FontArc, videos: Vec<&'a Layer>) -> Result<Self, String> {
        let videos = videos
            .into_iter()
            .map(|l| {
                let (path, start, _) = video_source(l);
                video::FrameReader::spawn(path, start, decode_size(l), false).map(|r| (l, r))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self { doc, images, font, videos, cache: HashMap::new() })
    }

    fn ensure_segments(&mut self, slide: u32) {
        if self.cache.contains_key(&slide) {
            return;
        }
        let (sw, _) = self.doc.slide_size();
        let ids: Vec<u64> = self.videos.iter().map(|(l, _)| l.id).collect();
        let mut segments = Vec::with_capacity(ids.len() + 1);
        let mut layers = self.doc.layers.iter().peekable();
        for k in 0..=ids.len() {
            let mut part = Vec::new();
            while let Some(l) = layers.next_if(|l| ids.get(k) != Some(&l.id)) {
                part.push(l);
            }
            layers.next(); // el propio video k
            let frames = Frames::new();
            let x0 = slide as f32 * sw;
            segments.push(render_layers(self.doc, part, self.images, self.font, &frames, x0, sw as u32, k == 0));
        }
        self.cache.insert(slide, segments);
    }

    /// Un fotograma de un slide de ancho, empezando en `x0` del lienzo.
    fn frame(&mut self, x0: u32) -> RgbaImage {
        let (sw, sh) = self.doc.format.size();
        let a = x0 / sw;
        let off = x0 - a * sw;
        let b = (off > 0 && a + 1 < self.doc.slides).then_some(a + 1);
        self.cache.retain(|&s, _| s + 1 >= a);
        self.ensure_segments(a);
        if let Some(b) = b {
            self.ensure_segments(b);
        }
        // Piezas: (slide, x de origen, x de destino, ancho).
        let mut pieces = vec![(a, off, 0, sw - off)];
        if let Some(b) = b {
            pieces.push((b, 0, sw - off, off));
        }
        let mut out = RgbaImage::from_pixel(sw, sh, Rgba(self.doc.background));
        let n = self.videos.len();
        for k in 0..=n {
            for &(slide, sx, dx, w) in &pieces {
                let seg = &self.cache[&slide][k];
                let piece = imageops::crop_imm(seg, sx, 0, w, sh).to_image();
                if k == 0 {
                    imageops::replace(&mut out, &piece, dx as i64, 0);
                } else {
                    imageops::overlay(&mut out, &piece, dx as i64, 0);
                }
            }
            if k < n {
                let (layer, reader) = &mut self.videos[k];
                if let Some(frame) = reader.next_frame() {
                    let mut l = (*layer).clone();
                    l.x -= x0 as f32;
                    draw_image(&mut out, frame, &l);
                }
            }
        }
        out
    }
}

/// El primer video con sonido (y sin silenciar) pone el audio.
fn pick_audio(videos: &[&Layer], duration: f32, looped: bool) -> Option<video::Audio> {
    videos.iter().find_map(|l| {
        let (path, start, muted) = video_source(l);
        let info = video::probe(path).ok()?;
        (!muted && info.has_audio).then(|| video::Audio {
            path: path.to_path_buf(),
            start,
            looped: looped || info.duration - start < duration,
        })
    })
}

fn frame_count(seconds: f32) -> u32 {
    ((seconds * video::FPS as f32).ceil() as u32).max(1)
}

/// Exporta un slide con video como MP4 (dura lo que el video más largo, hasta 60 s).
fn export_video_slide(
    doc: &Doc,
    images: &HashMap<PathBuf, Arc<RgbaImage>>,
    font: &FontArc,
    slide: u32,
    out: &Path,
    progress: &dyn Fn(f32),
) -> Result<(), String> {
    let (sw, sh) = doc.format.size();
    let x0 = slide * sw;
    let vids = videos_in(doc, x0 as f32, sw as f32);
    let duration = vids
        .iter()
        .map(|l| {
            let (path, start, _) = video_source(l);
            video::probe(path).map(|i| (i.duration - start).max(0.5))
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .fold(0.5_f32, f32::max)
        .min(video::MAX_SLIDE_SECONDS);
    let audio = pick_audio(&vids, duration, false);
    let mut comp = Compositor::new(doc, images, font, vids)?;
    let mut enc = video::Encoder::new(out, (sw, sh), duration, audio.as_ref())?;
    let frames = frame_count(duration);
    for f in 0..frames {
        enc.write(&comp.frame(x0))?;
        progress((f + 1) as f32 / frames as f32);
    }
    enc.finish()
}

pub struct ExportReport {
    pub slides: u32,
    /// Slides que salieron como video MP4.
    pub videos: u32,
    /// El panorama era demasiado ancho para JPG y se guardó en PNG.
    pub panorama_png: bool,
}

/// Exporta un archivo por slide (imagen, o MP4 si el slide tiene video) y
/// opcionalmente el panorama completo como imagen. Cada slide se dibuja por
/// separado, así la memoria no crece con el número de slides.
/// `progress` recibe la fracción completada (0 a 1).
pub fn export(
    doc: &Doc,
    images: &HashMap<PathBuf, Arc<RgbaImage>>,
    font: &FontArc,
    folder: &Path,
    format: ExportFormat,
    panorama: bool,
    progress: &dyn Fn(f32),
) -> Result<ExportReport, String> {
    let (sw, _) = doc.format.size();
    let ext = |f: ExportFormat| match f {
        ExportFormat::Jpg => "jpg",
        ExportFormat::Png => "png",
    };
    let total = (doc.slides + panorama as u32) as f32;
    let mut videos = 0;
    for i in 0..doc.slides {
        let x0 = (i * sw) as f32;
        if videos_in(doc, x0, sw as f32).is_empty() {
            let slide = render_region(doc, images, font, x0, sw);
            save_image(slide, &folder.join(slide_name(i, doc.slides, ext(format))), format)?;
        } else {
            let out = folder.join(slide_name(i, doc.slides, "mp4"));
            export_video_slide(doc, images, font, i, &out, &|f| progress((i as f32 + f) / total))?;
            videos += 1;
        }
        progress((i + 1) as f32 / total);
    }
    let mut panorama_png = false;
    if panorama {
        let width = doc.slides * sw;
        // Un JPG no puede medir más de 65 535 px: el panorama sale en PNG.
        let pano_format = if format == ExportFormat::Jpg && width > JPEG_MAX {
            panorama_png = true;
            ExportFormat::Png
        } else {
            format
        };
        // En el panorama (una imagen) los videos salen con su primer fotograma.
        let mut posters = Frames::new();
        for l in videos_in(doc, 0.0, width as f32) {
            let (path, start, _) = video_source(l);
            posters.insert(l.id, video::frame_at(path, start, decode_size(l))?);
        }
        let canvas = render_layers(doc, &doc.layers, images, font, &posters, 0.0, width, true);
        save_image(canvas, &folder.join(format!("panorama.{}", ext(pano_format))), pano_format)?;
        progress(1.0);
    }
    Ok(ExportReport { slides: doc.slides, videos, panorama_png })
}

/// Video vertical 9:16 (1080×1920) que recorre el carrusel slide a slide,
/// para Reels y TikTok. Cada slide se queda `seconds_per_slide` segundos y
/// luego se desliza al siguiente.
pub fn export_scroll_video(
    doc: &Doc,
    images: &HashMap<PathBuf, Arc<RgbaImage>>,
    font: &FontArc,
    out: &Path,
    seconds_per_slide: f32,
    progress: &dyn Fn(f32),
) -> Result<(), String> {
    const TRANSITION: f32 = 0.6;
    let (sw, sh) = doc.format.size();
    let (ow, oh) = (sw, 1920.max(sh));
    let hold = seconds_per_slide.max(0.5);
    let n = doc.slides;
    let duration = n as f32 * hold + (n - 1) as f32 * TRANSITION;
    let vids: Vec<&Layer> = doc.layers.iter().filter(|l| l.is_video() && !l.hidden).collect();
    let audio = pick_audio(&vids, duration, true);
    let mut comp = Compositor::new(doc, images, font, vids)?;
    let mut enc = video::Encoder::new(out, (ow, oh), duration, audio.as_ref())?;
    let frames = frame_count(duration);
    let top = ((oh - sh) / 2) as i64;
    for f in 0..frames {
        let t = f as f32 / video::FPS as f32;
        let pos = scroll_position(t, hold, TRANSITION, n);
        let x0 = ((pos * sw as f32).round() as u32).min((n - 1) * sw);
        let content = comp.frame(x0);
        let mut frame = RgbaImage::from_pixel(ow, oh, Rgba(doc.background));
        imageops::replace(&mut frame, &content, 0, top);
        enc.write(&frame)?;
        progress((f + 1) as f32 / frames as f32);
    }
    enc.finish()
}

/// Posición (en slides) en el instante `t`: quieto `hold` segundos en cada
/// slide y luego un deslizamiento suave de `transition` segundos.
pub fn scroll_position(t: f32, hold: f32, transition: f32, slides: u32) -> f32 {
    let step = hold + transition;
    let k = (t / step).floor();
    let local = t - k * step;
    let p = if local < hold {
        k
    } else {
        let x = ((local - hold) / transition).clamp(0.0, 1.0);
        k + x * x * (3.0 - 2.0 * x)
    };
    p.clamp(0.0, (slides - 1) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{fit, Crop, Edge, Format, Layer, TextAlign};

    fn font() -> FontArc {
        FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).unwrap()
    }

    fn text(s: &str, size: f32, align: TextAlign) -> Kind {
        Kind::Text { text: s.into(), size, color: [20, 20, 20, 255], align, background: None, shadow: false }
    }

    #[test]
    fn exporta_slides() {
        let out = std::env::var("CARRUSEL_TEST_OUT").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir().join("carrusel_test"));
        std::fs::create_dir_all(&out).unwrap();
        // Una "foto" panorámica en degradado que cruza las slides 1 y 2.
        let pano = RgbaImage::from_fn(3000, 1200, |x, y| Rgba([(x * 255 / 3000) as u8, 120, (y * 255 / 1200) as u8, 255]));
        let small = RgbaImage::from_pixel(800, 800, Rgba([250, 200, 40, 255]));
        let mut images = HashMap::new();
        images.insert(PathBuf::from("pano"), Arc::new(pano));
        images.insert(PathBuf::from("small"), Arc::new(small));
        let mut doc = Doc { format: Format::Portrait, slides: 3, background: [245, 240, 235, 255], ..Default::default() };
        let rect = fit(3000.0 / 1200.0, doc.slides_rect(0, 2), true);
        doc.layers.push(Layer::new(1, Kind::Image { path: "pano".into() }, rect));
        let mut rounded = Layer::new(2, Kind::Image { path: "small".into() }, (1700.0, 300.0, 700.0, 700.0));
        rounded.radius = 120.0;
        doc.layers.push(rounded);
        let mut t = Layer::new(3, text("Hola\nCarrusel", 120.0, TextAlign::Center), (2300.0, 1000.0, 0.0, 0.0));
        (t.w, t.h) = measure_text(&font(), "Hola\nCarrusel", 120.0);
        doc.layers.push(t);
        let n = export(&doc, &images, &font(), &out, ExportFormat::Png, true, &|_| {}).unwrap();
        assert_eq!(n.slides, 3);
        // Dibujar slide a slide da lo mismo que recortar el panorama.
        let whole = render_canvas(&doc, &images, &font());
        let s3 = render_region(&doc, &images, &font(), 2160.0, 1080);
        let reference = imageops::crop_imm(&whole, 2160, 0, 1080, 1350).to_image();
        let max_diff = s3
            .pixels()
            .zip(reference.pixels())
            .flat_map(|(a, b)| (0..3).map(move |c| (a[c] as i32 - b[c] as i32).abs()))
            .max()
            .unwrap();
        println!("diferencia máxima slide vs panorama: {max_diff}");
        assert!(max_diff <= 8, "diferencia máxima {max_diff}");
        let s1 = image::open(out.join("slide_01.png")).unwrap();
        assert_eq!((s1.width(), s1.height()), (1080, 1350));
        let p = image::open(out.join("panorama.png")).unwrap().to_rgba8();
        assert_eq!((p.width(), p.height()), (3240, 1350));
        // Continuidad: la última columna de la slide 1 y la primera de la 2 vienen de la misma foto.
        let s2 = image::open(out.join("slide_02.png")).unwrap().to_rgba8();
        let a = s1.to_rgba8().get_pixel(1079, 600).0;
        let b = s2.get_pixel(0, 600).0;
        assert!((a[0] as i32 - b[0] as i32).abs() <= 2, "{a:?} vs {b:?}");
        // Esquinas redondeadas: la esquina de la foto amarilla deja ver la panorámica,
        // pero el centro de su borde sí es amarillo.
        assert_ne!(p.get_pixel(1702, 302).0, [250, 200, 40, 255]);
        assert_eq!(p.get_pixel(2050, 302).0, [250, 200, 40, 255]);
    }

    #[test]
    fn recortar_antes_de_escalar_da_el_mismo_resultado() {
        // Foto con detalle: si el recorte se desalinea, los píxeles no coinciden.
        let src = RgbaImage::from_fn(400, 300, |x, y| Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]));
        // Capa dentro del lienzo (camino sin recorte) frente a la misma capa
        // asomando por la izquierda: la parte visible debe ser igual.
        let mut a = RgbaImage::from_pixel(1000, 600, Rgba([0, 0, 0, 255]));
        draw_image(&mut a, &src, &Layer::new(1, Kind::Image { path: "x".into() }, (100.0, 0.0, 800.0, 600.0)));
        let mut b = RgbaImage::from_pixel(600, 600, Rgba([0, 0, 0, 255]));
        draw_image(&mut b, &src, &Layer::new(1, Kind::Image { path: "x".into() }, (-300.0, 0.0, 800.0, 600.0)));
        let mut max_diff = 0;
        for y in 10..590 {
            for x in 10..490 {
                let pa = a.get_pixel(x + 400, y).0;
                let pb = b.get_pixel(x, y).0;
                for c in 0..3 {
                    max_diff = max_diff.max((pa[c] as i32 - pb[c] as i32).abs());
                }
            }
        }
        assert!(max_diff <= 40, "diferencia máxima {max_diff}");
    }

    #[test]
    fn capa_enorme_no_revienta_la_memoria() {
        // 200 000 × 200 000 px a tamaño completo serían 160 GB.
        let src = RgbaImage::from_pixel(1000, 1000, Rgba([10, 200, 30, 255]));
        let mut canvas = RgbaImage::from_pixel(1080, 1350, Rgba([255, 255, 255, 255]));
        let l = Layer::new(1, Kind::Image { path: "x".into() }, (-100_000.0, -100_000.0, 200_000.0, 200_000.0));
        draw_image(&mut canvas, &src, &l);
        assert_eq!(canvas.get_pixel(500, 500).0, [10, 200, 30, 255]);
    }

    #[test]
    fn exporta_solo_la_parte_recortada() {
        // Foto mitad roja (izquierda) y mitad azul (derecha).
        let src = RgbaImage::from_fn(200, 100, |x, _| if x < 100 { Rgba([255, 0, 0, 255]) } else { Rgba([0, 0, 255, 255]) });
        let mut l = Layer::new(1, Kind::Image { path: "x".into() }, (0.0, 0.0, 400.0, 200.0));
        // Recortar toda la mitad roja desde la izquierda.
        l.crop_edge(Edge::Left, 200.0);
        let mut canvas = RgbaImage::from_pixel(400, 200, Rgba([255, 255, 255, 255]));
        draw_image(&mut canvas, &src, &l);
        // Lo recortado deja ver el fondo; la parte azul sigue en su sitio.
        assert_eq!(canvas.get_pixel(100, 100).0, [255, 255, 255, 255]);
        assert_eq!(canvas.get_pixel(198, 100).0, [255, 255, 255, 255]);
        assert_eq!(canvas.get_pixel(300, 100).0, [0, 0, 255, 255]);

        // Mover la foto dentro del marco enseña otra parte en el mismo sitio.
        let mut m = Layer::new(1, Kind::Image { path: "x".into() }, (0.0, 0.0, 200.0, 200.0));
        m.crop = Crop { u0: 0.0, v0: 0.0, u1: 0.5, v1: 1.0 };
        let mut c2 = RgbaImage::from_pixel(400, 200, Rgba([255, 255, 255, 255]));
        draw_image(&mut c2, &src, &m);
        assert_eq!(c2.get_pixel(100, 100).0, [255, 0, 0, 255]);
        m.pan_crop(-1000.0, 0.0);
        let mut c3 = RgbaImage::from_pixel(400, 200, Rgba([255, 255, 255, 255]));
        draw_image(&mut c3, &src, &m);
        assert_eq!(c3.get_pixel(100, 100).0, [0, 0, 255, 255]);
        assert_eq!(c3.get_pixel(300, 100).0, [255, 255, 255, 255], "el marco no crece");
    }

    #[test]
    fn nombres_ordenables_con_muchos_slides() {
        assert_eq!(slide_name(0, 3, "jpg"), "slide_01.jpg");
        assert_eq!(slide_name(0, 150, "jpg"), "slide_001.jpg");
        assert_eq!(slide_name(149, 150, "png"), "slide_150.png");
    }

    #[test]
    fn exporta_muchos_slides() {
        let out = std::env::temp_dir().join("carrusel_test_muchos");
        let _ = std::fs::remove_dir_all(&out);
        std::fs::create_dir_all(&out).unwrap();
        let doc = Doc { format: Format::Square, slides: 45, ..Default::default() };
        let calls = std::cell::Cell::new(0);
        let r = export(&doc, &HashMap::new(), &font(), &out, ExportFormat::Jpg, false, &|_| calls.set(calls.get() + 1)).unwrap();
        assert_eq!(r.slides, 45);
        assert_eq!(calls.get(), 45);
        assert!(out.join("slide_45.jpg").exists());
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn exporta_slide_con_video_como_mp4() {
        let dir = std::env::temp_dir().join("carrusel_export_video");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let Some(clip) = video::tests::sample_video(&dir, "clip.mov", (640, 360), 1.0) else {
            eprintln!("sin ffmpeg: prueba omitida");
            return;
        };
        let mut doc = Doc { format: Format::Portrait, slides: 2, ..Default::default() };
        let mut v = Layer::new(1, Kind::Video { path: clip.clone(), start: 0.0, muted: false }, (0.0, 0.0, 10.0, 10.0));
        v.set_cover(640.0 / 360.0, doc.slides_rect(1, 1));
        v.border = 20.0;
        doc.layers.push(v);
        doc.layers.push(Layer::new(2, text("Encima", 80.0, TextAlign::Left), (1200.0, 100.0, 300.0, 96.0)));
        let r = export(&doc, &HashMap::new(), &font(), &dir, ExportFormat::Jpg, true, &|_| {}).unwrap();
        assert_eq!((r.slides, r.videos), (2, 1));
        assert!(dir.join("slide_01.jpg").exists(), "el slide sin video sale como foto");
        let info = video::probe(&dir.join("slide_02.mp4")).unwrap();
        assert_eq!((info.width, info.height), (1080, 1350));
        assert!((info.duration - 1.0).abs() < 0.15, "{}", info.duration);
        assert!(info.has_audio, "conserva el sonido del video");
        // Dentro del MP4 se ve el video (no el fondo) y el marco blanco en el borde.
        let mid = video::frame_at(&dir.join("slide_02.mp4"), 0.5, (1080, 1350)).unwrap();
        let center = mid.get_pixel(540, 675).0;
        assert!(center[..3] != [255, 255, 255], "el centro debería mostrar el video: {center:?}");
        assert!(mid.get_pixel(8, 675).0[..3].iter().all(|&c| c > 235), "falta el marco: {:?}", mid.get_pixel(8, 675));
        // El panorama (imagen) lleva el primer fotograma del video.
        let pano = image::open(dir.join("panorama.jpg")).unwrap().to_rgb8();
        assert_ne!(pano.get_pixel(1600, 900).0, [255, 255, 255]);
        // Y el marco blanco por dentro del borde del video.
        assert_eq!(pano.get_pixel(1090, 700).0.map(|c| c > 240), [true; 3]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn video_desplazable_vertical_para_tiktok() {
        let dir = std::env::temp_dir().join("carrusel_scroll_video");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if video::ffmpeg().is_none() {
            return;
        }
        let mut doc = Doc { format: Format::Portrait, slides: 2, ..Default::default() };
        let star = Kind::Shape { shape: crate::doc::ShapeKind::Star, fill: [255, 200, 0, 255], stroke: [0; 4], stroke_w: 0.0 };
        doc.layers.push(Layer::new(1, star, (800.0, 300.0, 600.0, 600.0)));
        let out = dir.join("reel.mp4");
        export_scroll_video(&doc, &HashMap::new(), &font(), &out, 0.5, &|_| {}).unwrap();
        let info = video::probe(&out).unwrap();
        assert_eq!((info.width, info.height), (1080, 1920));
        assert!((info.duration - 1.6).abs() < 0.15, "{}", info.duration);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn desplazamiento_se_detiene_en_cada_slide() {
        assert_eq!(scroll_position(0.0, 2.0, 0.6, 3), 0.0);
        assert_eq!(scroll_position(1.9, 2.0, 0.6, 3), 0.0);
        assert!((scroll_position(2.3, 2.0, 0.6, 3) - 0.5).abs() < 1e-3);
        assert_eq!(scroll_position(2.7, 2.0, 0.6, 3), 1.0);
        assert_eq!(scroll_position(100.0, 2.0, 0.6, 3), 2.0);
    }

    #[test]
    fn texto_con_fondo_y_emoji_se_exportan() {
        let mut doc = Doc { format: Format::Square, slides: 1, background: [255, 255, 255, 255], ..Default::default() };
        let mut t = Layer::new(1, text("Hola", 80.0, TextAlign::Left), (100.0, 100.0, 0.0, 0.0));
        (t.w, t.h) = measure_text(&font(), "Hola", 80.0);
        if let Kind::Text { background, .. } = &mut t.kind {
            *background = Some([0, 0, 0, 255]);
        }
        doc.layers.push(t);
        doc.layers.push(Layer::new(2, Kind::Emoji { emoji: "🔥".into(), sticker: true, style: crate::doc::EmojiStyle::Noto }, (500.0, 500.0, 400.0, 400.0)));
        let img = render_region(&doc, &HashMap::new(), &font(), 0.0, 1080);
        // El recuadro negro asoma por fuera de la caja del texto.
        assert_eq!(img.get_pixel(95, 98).0, [0, 0, 0, 255]);
        // El fuego naranja aparece en su sitio.
        let orange = (500..900).flat_map(|x| (500..900).map(move |y| (x, y))).any(|(x, y)| {
            let p = img.get_pixel(x, y).0;
            p[0] > 200 && p[1] > 80 && p[1] < 200 && p[2] < 80
        });
        assert!(orange);
    }

    /// Muestra de todos los elementos en una imagen, para revisarla a ojo:
    /// `CARRUSEL_TEST_OUT=carpeta cargo test muestra -- --ignored`
    #[test]
    #[ignore]
    fn muestra_de_elementos() {
        use crate::doc::{FrameKind, ShapeKind};
        let out = std::env::var("CARRUSEL_TEST_OUT").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
        std::fs::create_dir_all(&out).unwrap();
        let mut doc = Doc { format: Format::Portrait, slides: 3, background: [250, 244, 236, 255], ..Default::default() };
        let mut id = 0;
        let mut push = |doc: &mut Doc, kind: Kind, rect: (f32, f32, f32, f32)| {
            id += 1;
            doc.layers.push(Layer::new(id, kind, rect));
        };
        let photo = RgbaImage::from_fn(1200, 900, |x, y| Rgba([(60 + x / 8) as u8, (120 + y / 10) as u8, 200, 255]));
        let mut images = HashMap::new();
        images.insert(PathBuf::from("foto"), Arc::new(photo));
        let mut p = Layer::new(100, Kind::Image { path: "foto".into() }, (90.0, 120.0, 900.0, 675.0));
        p.border = 24.0;
        p.radius = 30.0;
        doc.layers.push(p);
        push(&mut doc, Kind::Frame { frame: FrameKind::Polaroid, color: [255, 255, 255, 255], thickness: 40.0 }, (60.0, 860.0, 420.0, 440.0));
        push(&mut doc, Kind::Frame { frame: FrameKind::Film, color: [20, 20, 20, 255], thickness: 36.0 }, (520.0, 860.0, 500.0, 300.0));
        for (i, shape) in ShapeKind::ALL.iter().enumerate() {
            let (col, row) = (i % 3, i / 3);
            let rect = if *shape == ShapeKind::Line { (1160.0 + col as f32 * 300.0, 120.0 + row as f32 * 300.0 + 120.0, 260.0, 16.0) } else { (1160.0 + col as f32 * 300.0, 120.0 + row as f32 * 300.0, 260.0, 260.0) };
            push(&mut doc, Kind::Shape { shape: *shape, fill: [255, 64, 129, 255], stroke: [30, 30, 30, 255], stroke_w: 8.0 }, rect);
        }
        for (i, e) in ["😀", "🔥", "❤️", "🐱", "🍕", "🇲🇽"].iter().enumerate() {
            let x = 2240.0 + (i % 3) as f32 * 320.0;
            let y = 100.0 + (i / 3) as f32 * 340.0;
            let style = if i < 3 { crate::doc::EmojiStyle::Twemoji } else { crate::doc::EmojiStyle::Noto };
            push(&mut doc, Kind::Emoji { emoji: (*e).into(), sticker: i % 2 == 1, style }, (x, y, 300.0, 300.0));
        }
        let f = font();
        for (i, (txt, bg, shadow)) in [("Etiqueta", Some([255, 64, 129, 255]), false), ("Sombra", None, true)].into_iter().enumerate() {
            let mut t = Layer::new(200 + i as u64, Kind::Text { text: txt.into(), size: 90.0, color: if bg.is_some() { [255; 4] } else { [40, 40, 40, 255] }, align: TextAlign::Center, background: bg, shadow }, (2300.0, 900.0 + i as f32 * 180.0, 0.0, 0.0));
            (t.w, t.h) = measure_text(&f, txt, 90.0);
            doc.layers.push(t);
        }
        let img = render_canvas(&doc, &images, &f);
        img.save(out.join("muestra.png")).unwrap();
    }

    #[test]
    fn capas_ocultas_no_se_exportan() {
        let mut doc = Doc { format: Format::Square, slides: 1, background: [255, 255, 255, 255], ..Default::default() };
        let mut star = Layer::new(1, Kind::Shape { shape: crate::doc::ShapeKind::Rect, fill: [255, 0, 0, 255], stroke: [0; 4], stroke_w: 0.0 }, (0.0, 0.0, 1080.0, 1080.0));
        doc.layers.push(star.clone());
        assert_eq!(render_region(&doc, &HashMap::new(), &font(), 0.0, 1080).get_pixel(540, 540).0, [255, 0, 0, 255]);
        star.hidden = true;
        doc.layers[0] = star;
        assert_eq!(render_region(&doc, &HashMap::new(), &font(), 0.0, 1080).get_pixel(540, 540).0, [255, 255, 255, 255]);
    }

    #[test]
    fn opacidad_mezcla_con_el_fondo() {
        let src = RgbaImage::from_pixel(10, 10, Rgba([0, 0, 0, 255]));
        let mut canvas = RgbaImage::from_pixel(10, 10, Rgba([255, 255, 255, 255]));
        let mut l = Layer::new(1, Kind::Image { path: "x".into() }, (0.0, 0.0, 10.0, 10.0));
        l.opacity = 0.5;
        draw_image(&mut canvas, &src, &l);
        let v = canvas.get_pixel(5, 5).0[0];
        assert!((120..=136).contains(&v), "{v}");
    }

    #[test]
    fn texto_alineado_a_la_derecha() {
        let f = font();
        let s = "Hola\nun texto largo";
        let (w, h) = measure_text(&f, s, 60.0);
        let ink_x = |align| {
            let mut c = RgbaImage::from_pixel(1000, 300, Rgba([255, 255, 255, 255]));
            draw_text(&mut c, &f, &Layer::new(1, text(s, 60.0, align), (100.0, 0.0, w, h)));
            // Primera columna con tinta en la línea corta ("Hola").
            (0..1000).find(|&x| (0..60).any(|y| c.get_pixel(x, y).0[0] < 128)).unwrap() as f32
        };
        let left = ink_x(TextAlign::Left);
        let right = ink_x(TextAlign::Right);
        let shift = w - line_width(&f, "Hola", 60.0);
        assert!((right - left - shift).abs() < 3.0, "izq {left}, der {right}, esperado +{shift}");
    }
}
