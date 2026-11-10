//! Capas vectoriales (emojis/calcomanías, formas y marcos): se describen como
//! SVG y se rasterizan con resvg, igual en la vista del editor y al exportar.

use crate::doc::{EmojiStyle, FrameKind, Kind, ShapeKind};
use std::collections::HashMap;
use image::RgbaImage;
use resvg::{tiny_skia, usvg};
use std::fmt::Write;
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Biblioteca de emojis (Twemoji + nombres en español de Unicode CLDR)

pub struct EmojiEntry {
    pub emoji: &'static str,
    pub group: &'static str,
    pub name: &'static str,
    /// Nombre y palabras clave sin tildes y en minúsculas, para buscar.
    pub search: String,
    svg: &'static str,
}

pub const GROUPS: [&str; 9] =
    ["Caras", "Personas", "Naturaleza", "Comida", "Viajes", "Actividades", "Objetos", "Símbolos", "Banderas"];

static EMOJI_TSV: &str = include_str!("../assets/emoji.tsv");

pub fn emojis() -> &'static [EmojiEntry] {
    static LIST: OnceLock<Vec<EmojiEntry>> = OnceLock::new();
    LIST.get_or_init(|| {
        EMOJI_TSV
            .lines()
            .filter_map(|line| {
                let mut f = line.split('\t');
                let (emoji, group, name, keywords, svg) = (f.next()?, f.next()?, f.next()?, f.next()?, f.next()?);
                let search = fold(&format!("{name} {}", keywords.replace('|', " ")));
                Some(EmojiEntry { emoji, group, name, search, svg })
            })
            .collect()
    })
}

/// Google Noto Emoji 2D (Apache 2.0), comprimido con zlib para no inflar la app.
static NOTO_ZLIB: &[u8] = include_bytes!("../assets/emoji_noto.zlib");

fn noto() -> &'static HashMap<String, String> {
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    MAP.get_or_init(|| {
        let data = miniz_oxide::inflate::decompress_to_vec_zlib(NOTO_ZLIB).unwrap_or_default();
        String::from_utf8_lossy(&data)
            .lines()
            .filter_map(|l| l.split_once('\t').map(|(e, svg)| (e.to_string(), svg.to_string())))
            .collect()
    })
}

/// SVG de un emoji en el estilo pedido; si Noto no lo tiene, el de Twemoji.
pub fn emoji_svg(emoji: &str, style: EmojiStyle) -> Option<&'static str> {
    let twemoji = || emojis().iter().find(|e| e.emoji == emoji).map(|e| e.svg);
    match style {
        EmojiStyle::Twemoji => twemoji(),
        EmojiStyle::Noto => noto().get(emoji).map(String::as_str).or_else(twemoji),
    }
}

/// Minúsculas y sin tildes: «Corazón» → «corazon».
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

/// Coloca un SVG completo dentro de otro, en el cuadrado (x, y, lado). Se
/// anida entero para conservar sus espacios de nombres y definiciones.
fn nest_svg(src: &str, x: f32, y: f32, side: f32) -> Option<String> {
    let start = src.find("<svg")?;
    Some(format!("<svg x=\"{x}\" y=\"{y}\" width=\"{side}\" height=\"{side}\"{}", &src[start + 4..]))
}

// ---------------------------------------------------------------------------
// SVG de cada capa vectorial, en unidades del lienzo (0..w × 0..h)

fn hex(c: [u8; 4]) -> String {
    format!("#{:02x}{:02x}{:02x}\" fill-opacity=\"{:.3}", c[0], c[1], c[2], c[3] as f32 / 255.0)
}

fn paint(fill: [u8; 4], stroke: [u8; 4], stroke_w: f32) -> String {
    let mut s = if fill[3] == 0 { "fill=\"none\"".to_string() } else { format!("fill=\"{}\"", hex(fill)) };
    if stroke[3] > 0 && stroke_w > 0.0 {
        let _ = write!(
            s,
            " stroke=\"#{:02x}{:02x}{:02x}\" stroke-opacity=\"{:.3}\" stroke-width=\"{stroke_w}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"",
            stroke[0],
            stroke[1],
            stroke[2],
            stroke[3] as f32 / 255.0
        );
    }
    s
}

/// Polígono a partir de puntos en coordenadas 0..1 dentro del área útil.
fn polygon(pts: &[(f32, f32)], map: &dyn Fn(f32, f32) -> (f32, f32)) -> String {
    let mut d = String::new();
    for (i, &(u, v)) in pts.iter().enumerate() {
        let (x, y) = map(u, v);
        let _ = write!(d, "{}{x:.2} {y:.2} ", if i == 0 { "M" } else { "L" });
    }
    d + "Z"
}

fn star_points(n: usize, inner: f32) -> Vec<(f32, f32)> {
    (0..n * 2)
        .map(|i| {
            let r = if i % 2 == 0 { 0.5 } else { 0.5 * inner };
            let a = std::f32::consts::PI * i as f32 / n as f32 - std::f32::consts::FRAC_PI_2;
            (0.5 + r * a.cos(), 0.5 + r * a.sin())
        })
        .collect()
}

fn shape_body(shape: ShapeKind, w: f32, h: f32, inset: f32, attrs: &str) -> String {
    let (iw, ih) = ((w - 2.0 * inset).max(1.0), (h - 2.0 * inset).max(1.0));
    let map = |u: f32, v: f32| (inset + u * iw, inset + v * ih);
    let path = |d: String| format!("<path d=\"{d}\" {attrs}/>");
    match shape {
        ShapeKind::Rect => format!("<rect x=\"{inset}\" y=\"{inset}\" width=\"{iw}\" height=\"{ih}\" {attrs}/>"),
        ShapeKind::RoundRect => {
            let r = iw.min(ih) * 0.18;
            format!("<rect x=\"{inset}\" y=\"{inset}\" width=\"{iw}\" height=\"{ih}\" rx=\"{r}\" {attrs}/>")
        }
        ShapeKind::Ellipse => format!(
            "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" {attrs}/>",
            w / 2.0,
            h / 2.0,
            iw / 2.0,
            ih / 2.0
        ),
        ShapeKind::Triangle => path(polygon(&[(0.5, 0.0), (1.0, 1.0), (0.0, 1.0)], &map)),
        ShapeKind::Star => path(polygon(&star_points(5, 0.45), &map)),
        ShapeKind::Burst => path(polygon(&star_points(14, 0.78), &map)),
        ShapeKind::Arrow => path(polygon(
            &[(0.0, 0.32), (0.6, 0.32), (0.6, 0.05), (1.0, 0.5), (0.6, 0.95), (0.6, 0.68), (0.0, 0.68)],
            &map,
        )),
        ShapeKind::Line => {
            // La línea usa el color de relleno como trazo y ocupa todo el alto de la caja.
            format!(
                "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" {attrs}/>",
                h / 2.0,
                h / 2.0,
                (w - h / 2.0).max(h / 2.0),
                h / 2.0
            )
        }
        ShapeKind::Heart => {
            let p = |u: f32, v: f32| {
                let (x, y) = map(u, v);
                format!("{x:.2} {y:.2}")
            };
            path(format!(
                "M{} C{} {} {} C{} {} {} C{} {} {} C{} {} {} Z",
                p(0.5, 0.25),
                p(0.5, 0.05),
                p(0.0, 0.0),
                p(0.0, 0.32),
                p(0.0, 0.6),
                p(0.35, 0.78),
                p(0.5, 1.0),
                p(0.65, 0.78),
                p(1.0, 0.6),
                p(1.0, 0.32),
                p(1.0, 0.0),
                p(0.5, 0.05),
                p(0.5, 0.25)
            ))
        }
        ShapeKind::Bubble => {
            let p = |u: f32, v: f32| {
                let (x, y) = map(u, v);
                format!("{x:.2} {y:.2}")
            };
            // Cuerpo redondeado (78 % de alto) con la cola abajo a la izquierda.
            let r = 0.12;
            let b = 0.78;
            path(format!(
                "M{} L{} Q{} {} L{} Q{} {} L{} L{} L{} L{} Q{} {} L{} Q{} {} Z",
                p(r, 0.0),
                p(1.0 - r, 0.0),
                p(1.0, 0.0),
                p(1.0, r),
                p(1.0, b - r),
                p(1.0, b),
                p(1.0 - r, b),
                p(0.36, b),
                p(0.14, 1.0),
                p(0.2, b),
                p(r, b),
                p(0.0, b),
                p(0.0, b - r),
                p(0.0, r),
                p(0.0, 0.0),
                p(r, 0.0)
            ))
        }
    }
}

fn frame_body(frame: FrameKind, w: f32, h: f32, t: f32, color: [u8; 4]) -> String {
    let t = t.clamp(1.0, w.min(h) / 2.0);
    let fill = format!("fill=\"{}\"", hex(color));
    let stroke = |sw: f32| paint([0; 4], color, sw).replace("stroke-linecap=\"round\"", "stroke-linecap=\"butt\"");
    let rect = |inset: f32, sw: f32, extra: &str| {
        format!(
            "<rect x=\"{inset}\" y=\"{inset}\" width=\"{}\" height=\"{}\" {} {extra}/>",
            (w - 2.0 * inset).max(0.0),
            (h - 2.0 * inset).max(0.0),
            stroke(sw)
        )
    };
    match frame {
        FrameKind::Simple => rect(t / 2.0, t, ""),
        FrameKind::Double => {
            let line = t * 0.3;
            rect(line / 2.0, line, "") + &rect(t - line / 2.0, line, "")
        }
        FrameKind::Dashed => rect(t / 2.0, t, &format!("stroke-dasharray=\"{} {}\"", t * 2.5, t * 1.5)),
        FrameKind::Polaroid => {
            let bottom = (t * 3.5).min(h / 2.0);
            format!(
                "<path fill-rule=\"evenodd\" {fill} d=\"M0 0H{w}V{h}H0Z M{t} {t}V{}H{}V{t}Z\"/>",
                h - bottom,
                w - t
            )
        }
        FrameKind::Film => {
            let band = (t * 1.6).min(h / 2.0);
            let hole_w = band * 0.55;
            let hole_h = band * 0.45;
            let step = band * 1.1;
            let mut d = format!("M0 0H{w}V{band}H0Z M0 {}H{w}V{h}H0Z", h - band);
            let mut x = step / 2.0 - hole_w / 2.0;
            while x + hole_w <= w {
                for y in [(band - hole_h) / 2.0, h - band + (band - hole_h) / 2.0] {
                    let _ = write!(d, " M{x:.2} {y:.2}h{hole_w:.2}v{hole_h:.2}h{:.2}Z", -hole_w);
                }
                x += step;
            }
            format!("<path fill-rule=\"evenodd\" {fill} d=\"{d}\"/>")
        }
        FrameKind::Corners => {
            let len = w.min(h) * 0.18;
            let o = t / 2.0;
            let d = format!(
                "M{o} {} V{o} H{} M{} {o} H{} V{} M{} {} V{} H{} M{} {} H{o} V{}",
                o + len,
                o + len,
                w - o - len,
                w - o,
                o + len,
                w - o,
                h - o - len,
                h - o,
                w - o - len,
                o + len,
                h - o,
                h - o - len
            );
            format!("<path d=\"{d}\" {}/>", stroke(t).replace("stroke-linecap=\"butt\"", "stroke-linecap=\"square\""))
        }
    }
}

/// SVG completo de una capa vectorial de tamaño `w × h`, o `None` si no lo es.
pub fn layer_svg(kind: &Kind, w: f32, h: f32) -> Option<String> {
    let (w, h) = (w.max(1.0), h.max(1.0));
    let body = match kind {
        Kind::Emoji { emoji, sticker, style } => {
            let src = emoji_svg(emoji, *style)?;
            let side = w.min(h);
            if *sticker {
                let r = side * 0.045;
                let pad = r * 2.2;
                format!(
                    concat!(
                        "<defs><filter id=\"st\" x=\"-20%\" y=\"-20%\" width=\"140%\" height=\"140%\" color-interpolation-filters=\"sRGB\">",
                        "<feMorphology in=\"SourceAlpha\" operator=\"dilate\" radius=\"{r}\" result=\"d\"/>",
                        "<feFlood flood-color=\"#ffffff\"/><feComposite in2=\"d\" operator=\"in\" result=\"outline\"/>",
                        "<feGaussianBlur in=\"d\" stdDeviation=\"{blur}\"/><feOffset dy=\"{dy}\" result=\"b\"/>",
                        "<feFlood flood-color=\"#000000\" flood-opacity=\"0.35\"/><feComposite in2=\"b\" operator=\"in\" result=\"shadow\"/>",
                        "<feMerge><feMergeNode in=\"shadow\"/><feMergeNode in=\"outline\"/><feMergeNode in=\"SourceGraphic\"/></feMerge>",
                        "</filter></defs>",
                        "<g filter=\"url(#st)\">{inner}</g>"
                    ),
                    r = r,
                    blur = r * 0.6,
                    dy = r * 0.5,
                    inner = nest_svg(src, (w - side) / 2.0 + pad, (h - side) / 2.0 + pad, side - 2.0 * pad)?
                )
            } else {
                nest_svg(src, (w - side) / 2.0, (h - side) / 2.0, side)?
            }
        }
        Kind::Shape { shape, fill, stroke, stroke_w } => {
            if *shape == ShapeKind::Line {
                // Una línea es solo trazo: su grosor es el alto de la caja.
                let attrs = paint([0; 4], *fill, h);
                shape_body(*shape, w, h, 0.0, &attrs)
            } else {
                let sw = if stroke[3] > 0 { *stroke_w } else { 0.0 };
                shape_body(*shape, w, h, sw / 2.0, &paint(*fill, *stroke, sw))
            }
        }
        Kind::Frame { frame, color, thickness } => frame_body(*frame, w, h, *thickness, *color),
        _ => return None,
    };
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">{body}</svg>"
    ))
}

/// Dibuja un SVG en una imagen de `size` píxeles: cada unidad del SVG mide
/// `scale` píxeles y el punto `offset` (en píxeles) queda en la esquina.
pub fn rasterize(svg: &str, scale: f32, offset: (f32, f32), size: (u32, u32)) -> Option<RgbaImage> {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(size.0.max(1), size.1.max(1))?;
    let transform = tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, -offset.0, -offset.1);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    // tiny-skia trabaja con alfa premultiplicado; image espera alfa normal.
    let mut data = pixmap.take();
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    RgbaImage::from_raw(size.0.max(1), size.1.max(1), data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque_pixels(img: &RgbaImage) -> usize {
        img.pixels().filter(|p| p[3] > 128).count()
    }

    #[test]
    fn la_biblioteca_trae_emojis_con_nombre_en_espanol() {
        let list = emojis();
        assert!(list.len() > 1500, "{}", list.len());
        let heart = list.iter().find(|e| e.emoji == "❤️").expect("corazón rojo");
        assert_eq!(heart.group, "Caras"); // Unicode agrupa los corazones con las emociones
        assert!(heart.search.contains("corazon"), "{}", heart.search);
        assert!(list.iter().any(|e| e.search.contains("fuego")));
        for g in GROUPS {
            assert!(list.iter().any(|e| e.group == g), "falta la categoría {g}");
        }
    }

    #[test]
    fn todos_los_emojis_se_dibujan_en_ambos_estilos() {
        assert_eq!(noto().len(), emojis().len(), "cada emoji tiene su versión Noto");
        for style in EmojiStyle::ALL {
            for e in emojis() {
                let svg = layer_svg(&Kind::Emoji { emoji: e.emoji.into(), sticker: false, style }, 36.0, 36.0).unwrap();
                let img = rasterize(&svg, 1.0, (0.0, 0.0), (36, 36))
                    .unwrap_or_else(|| panic!("{} ({style:?}) no se dibuja", e.emoji));
                assert!(opaque_pixels(&img) > 0, "{} ({style:?}) sale vacío", e.emoji);
            }
        }
    }

    #[test]
    fn noto_y_twemoji_son_distintos() {
        let draw = |style| {
            let k = Kind::Emoji { emoji: "😀".into(), sticker: false, style };
            rasterize(&layer_svg(&k, 128.0, 128.0).unwrap(), 1.0, (0.0, 0.0), (128, 128)).unwrap()
        };
        assert_ne!(draw(EmojiStyle::Twemoji), draw(EmojiStyle::Noto));
    }

    #[test]
    fn calcomania_agrega_borde_blanco() {
        let plain = Kind::Emoji { emoji: "🔥".into(), sticker: false, style: EmojiStyle::Noto };
        let sticker = Kind::Emoji { emoji: "🔥".into(), sticker: true, style: EmojiStyle::Noto };
        let a = rasterize(&layer_svg(&plain, 400.0, 400.0).unwrap(), 1.0, (0.0, 0.0), (400, 400)).unwrap();
        let b = rasterize(&layer_svg(&sticker, 400.0, 400.0).unwrap(), 1.0, (0.0, 0.0), (400, 400)).unwrap();
        let white = |img: &RgbaImage| img.pixels().filter(|p| p[3] > 200 && p[0] > 245 && p[1] > 245 && p[2] > 245).count();
        assert!(white(&b) > white(&a) + 1000, "el borde blanco no aparece");
    }

    #[test]
    fn formas_y_marcos_se_dibujan() {
        for shape in ShapeKind::ALL {
            let k = Kind::Shape { shape, fill: [255, 64, 129, 255], stroke: [0, 0, 0, 255], stroke_w: 8.0 };
            let img = rasterize(&layer_svg(&k, 300.0, 200.0).unwrap(), 1.0, (0.0, 0.0), (300, 200)).unwrap();
            assert!(opaque_pixels(&img) > 1000, "{shape:?} sale vacía");
        }
        for frame in FrameKind::ALL {
            let k = Kind::Frame { frame, color: [255, 255, 255, 255], thickness: 30.0 };
            let img = rasterize(&layer_svg(&k, 400.0, 500.0).unwrap(), 1.0, (0.0, 0.0), (400, 500)).unwrap();
            assert!(opaque_pixels(&img) > 1000, "{frame:?} sale vacío");
            // El centro de un marco queda libre para ver la foto.
            assert_eq!(img.get_pixel(200, 230)[3], 0, "{frame:?} tapa el centro");
        }
    }

    #[test]
    fn busqueda_sin_tildes() {
        assert_eq!(fold("Corazón Rojo"), "corazon rojo");
    }
}
