//! Carga de imágenes, medición de texto y exportación de slides.

use crate::doc::{Doc, Kind, Layer, LINE_H};
use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use image::{imageops, DynamicImage, ImageDecoder, ImageReader, Rgba, RgbaImage};
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

fn draw_text(canvas: &mut RgbaImage, font: &FontArc, l: &Layer) {
    let Kind::Text { text, size, color, align } = &l.kind else { return };
    let size = *size;
    let scale = PxScale::from(size);
    let sf = font.as_scaled(scale);
    let mut color = *color;
    color[3] = (color[3] as f32 * l.opacity.clamp(0.0, 1.0)).round() as u8;
    let (cw, ch) = (canvas.width() as i32, canvas.height() as i32);
    for (i, line) in text.split('\n').enumerate() {
        let x = l.x + align.offset(l.w, line_width(font, line, size));
        let mut caret = point(x, l.y + i as f32 * size * LINE_H + sf.ascent());
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

/// Cuánto cubre el rectángulo redondeado de la capa el punto (px, py), de 0 a 1,
/// con medio píxel de suavizado en el borde.
fn rounded_coverage(px: f32, py: f32, l: &Layer) -> f32 {
    let r = l.radius.clamp(0.0, l.w.min(l.h) / 2.0);
    let (hx, hy) = (l.w / 2.0, l.h / 2.0);
    let qx = (px - (l.x + hx)).abs() - (hx - r);
    let qy = (py - (l.y + hy)).abs() - (hy - r);
    let dist = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
    (0.5 - dist).clamp(0.0, 1.0)
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
    let mut piece = imageops::resize(&crop, tw, th, imageops::FilterType::CatmullRom);

    let opacity = l.opacity.clamp(0.0, 1.0);
    if l.radius > 0.0 || opacity < 1.0 || !l.crop.is_full() {
        for (px, py, p) in piece.enumerate_pixels_mut() {
            let cov = rounded_coverage(tx + px as f32 + 0.5, ty + py as f32 + 0.5, l) * opacity;
            p[3] = (p[3] as f32 * cov).round() as u8;
        }
    }
    imageops::overlay(canvas, &piece, tx as i64, ty as i64);
}

/// Dibuja la franja del lienzo que empieza en `x0` y mide `w` píxeles de ancho
/// (un slide, varios o el panorama entero) a resolución final.
pub fn render_region(doc: &Doc, images: &HashMap<PathBuf, Arc<RgbaImage>>, font: &FontArc, x0: f32, w: u32) -> RgbaImage {
    let (_, ch) = doc.canvas_size();
    let mut canvas = RgbaImage::from_pixel(w, ch as u32, Rgba(doc.background));
    let x1 = x0 + w as f32;
    for layer in &doc.layers {
        // Las fotos que no tocan esta franja se saltan (los textos pueden
        // sobresalir un poco de su caja, así que siempre se dibujan).
        if layer.is_image() && (layer.x + layer.w <= x0 || layer.x >= x1) {
            continue;
        }
        let mut l = layer.clone();
        l.x -= x0;
        match &l.kind {
            Kind::Image { path } => {
                if let Some(src) = images.get(path) {
                    draw_image(&mut canvas, src, &l);
                }
            }
            Kind::Text { .. } => draw_text(&mut canvas, font, &l),
        }
    }
    canvas
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

/// Nombre de cada archivo: slide_01, slide_02… (con más ceros si hacen falta,
/// para que se ordenen bien: slide_001 … slide_150).
pub fn slide_name(i: u32, total: u32, ext: &str) -> String {
    let digits = total.to_string().len().max(2);
    format!("slide_{:0digits$}.{ext}", i + 1)
}

/// Exporta un archivo por slide y opcionalmente el panorama completo.
/// Cada slide se dibuja por separado, así la memoria no crece con el número
/// de slides. `progress(hechas, total)` se llama tras cada archivo.
pub fn export(
    doc: &Doc,
    images: &HashMap<PathBuf, Arc<RgbaImage>>,
    font: &FontArc,
    folder: &Path,
    format: ExportFormat,
    panorama: bool,
    progress: &dyn Fn(u32, u32),
) -> Result<ExportReport, String> {
    let (sw, _) = doc.format.size();
    let ext = |f: ExportFormat| match f {
        ExportFormat::Jpg => "jpg",
        ExportFormat::Png => "png",
    };
    let save = |img: RgbaImage, name: String, format: ExportFormat| -> Result<(), String> {
        let path = folder.join(&name);
        let err = |e: &dyn std::fmt::Display| format!("{name}: {e}");
        match format {
            ExportFormat::Png => img.save(&path).map_err(|e| err(&e)),
            ExportFormat::Jpg => {
                let rgb = DynamicImage::ImageRgba8(img).to_rgb8();
                let file = std::fs::File::create(&path).map_err(|e| err(&e))?;
                let mut w = std::io::BufWriter::new(file);
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, 95)
                    .encode_image(&rgb)
                    .map_err(|e| err(&e))
            }
        }
    };
    let total = doc.slides + panorama as u32;
    for i in 0..doc.slides {
        let slide = render_region(doc, images, font, (i * sw) as f32, sw);
        save(slide, slide_name(i, doc.slides, ext(format)), format)?;
        progress(i + 1, total);
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
        let canvas = render_region(doc, images, font, 0.0, width);
        save(canvas, format!("panorama.{}", ext(pano_format)), pano_format)?;
        progress(total, total);
    }
    Ok(ExportReport { slides: doc.slides, panorama_png })
}

pub struct ExportReport {
    pub slides: u32,
    /// El panorama era demasiado ancho para JPG y se guardó en PNG.
    pub panorama_png: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{fit, Crop, Edge, Format, Layer, TextAlign};

    fn font() -> FontArc {
        FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).unwrap()
    }

    fn text(s: &str, size: f32, align: TextAlign) -> Kind {
        Kind::Text { text: s.into(), size, color: [20, 20, 20, 255], align }
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
        let n = export(&doc, &images, &font(), &out, ExportFormat::Png, true, &|_, _| {}).unwrap();
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
        let r = export(&doc, &HashMap::new(), &font(), &out, ExportFormat::Jpg, false, &|_, _| calls.set(calls.get() + 1)).unwrap();
        assert_eq!(r.slides, 45);
        assert_eq!(calls.get(), 45);
        assert!(out.join("slide_45.jpg").exists());
        let _ = std::fs::remove_dir_all(&out);
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
