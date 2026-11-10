//! Colección propia de stickers estilo scrapbook (diseños originales de
//! Carrusel): cintas washi, papeles, etiquetas, clips, garabatos, naturaleza y
//! sellos. Se generan como SVG con un trazo «hecho a mano» reproducible.

use crate::vector::fold;
use std::fmt::Write;
use std::sync::OnceLock;

pub const CATEGORIES: [&str; 7] = ["Cintas", "Papeles", "Etiquetas", "Clips", "Garabatos", "Naturaleza", "Sellos"];

pub struct Sticker {
    pub id: String,
    pub name: String,
    pub category: &'static str,
    /// Nombre, categoría y palabras clave sin tildes, para buscar.
    pub search: String,
    pub svg: String,
}

/// Colores pastel de scrapbook: (nombre, color de relleno, color oscuro a juego).
const PALETTE: [(&str, &str, &str); 8] = [
    ("rosa", "#f5b3c6", "#c9587a"),
    ("durazno", "#f8c8a4", "#cf7a45"),
    ("amarillo", "#f6df86", "#b9952a"),
    ("menta", "#b6e0c6", "#4f9a6c"),
    ("celeste", "#a8d3ef", "#3f7fb2"),
    ("lavanda", "#cdb9ec", "#7b5bb8"),
    ("kraft", "#cfa77c", "#8a6238"),
    ("crema", "#f4ecd8", "#a8946a"),
];

/// Pseudoaleatorio reproducible: el mismo sticker sale siempre igual.
struct Rng(u64);

impl Rng {
    fn new(seed: &str) -> Self {
        Rng(seed.bytes().fold(0xcbf29ce484222325, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3)))
    }
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % 10_000) as f32 / 10_000.0
    }
    /// Valor entre -a y a.
    fn j(&mut self, a: f32) -> f32 {
        (self.next() * 2.0 - 1.0) * a
    }
}

fn svg(w: f32, h: f32, defs: &str, body: &str) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\"><defs>{SHADOW}{PAPER}{defs}</defs>{body}</svg>")
}

/// Sombra suave para objetos «pegados» en la página.
const SHADOW: &str = "<filter id=\"sh\" x=\"-15%\" y=\"-15%\" width=\"130%\" height=\"140%\"><feDropShadow dx=\"0\" dy=\"3\" stdDeviation=\"3\" flood-color=\"#000\" flood-opacity=\"0.22\"/></filter>";
/// Textura de papel: grano fino y sombra suave.
const PAPER: &str = concat!(
    "<filter id=\"pp\" x=\"-10%\" y=\"-10%\" width=\"120%\" height=\"130%\">",
    "<feTurbulence type=\"fractalNoise\" baseFrequency=\"0.8\" numOctaves=\"2\" seed=\"3\" result=\"n\"/>",
    "<feColorMatrix in=\"n\" type=\"matrix\" values=\"0 0 0 0 0  0 0 0 0 0  0 0 0 0 0  0 0 0 0.09 0\" result=\"g\"/>",
    "<feComposite in=\"g\" in2=\"SourceGraphic\" operator=\"in\" result=\"grain\"/>",
    "<feMerge result=\"m\"><feMergeNode in=\"SourceGraphic\"/><feMergeNode in=\"grain\"/></feMerge>",
    "<feDropShadow in=\"m\" dx=\"0\" dy=\"3\" stdDeviation=\"3\" flood-color=\"#000\" flood-opacity=\"0.2\"/>",
    "</filter>"
);

/// Borde rasgado: puntos irregulares a lo largo de un segmento.
fn torn(rng: &mut Rng, (x0, y0): (f32, f32), (x1, y1): (f32, f32), steps: usize, amp: f32) -> String {
    let mut d = String::new();
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = (dx * dx + dy * dy).sqrt().max(1.0);
        let (nx, ny) = (-dy / len, dx / len);
        let a = if i == steps { 0.0 } else { rng.j(amp) };
        let _ = write!(d, " L{:.1} {:.1}", x0 + dx * t + nx * a, y0 + dy * t + ny * a);
    }
    d
}

/// Trazo «a mano»: une los puntos con curvas y un poco de temblor.
fn hand(rng: &mut Rng, pts: &[(f32, f32)], wobble: f32) -> String {
    let mut d = String::new();
    for (i, &(x, y)) in pts.iter().enumerate() {
        let (x, y) = (x + rng.j(wobble), y + rng.j(wobble));
        if i == 0 {
            let _ = write!(d, "M{x:.1} {y:.1}");
        } else {
            let (px, py) = pts[i - 1];
            let _ = write!(d, " Q{:.1} {:.1} {x:.1} {y:.1}", (px + x) / 2.0 + rng.j(wobble), (py + y) / 2.0 + rng.j(wobble));
        }
    }
    d
}

fn ink(d: &str, color: &str, width: f32) -> String {
    format!("<path d=\"{d}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{width}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>")
}

// ----- Cintas washi ---------------------------------------------------------

fn washi(rng: &mut Rng, fill: &str, dark: &str, pattern: &str) -> String {
    let (w, h) = (320.0, 84.0);
    // Extremos dentados como la cinta cortada con los dedos.
    let mut d = String::from("M8 4");
    d += &torn(rng, (8.0, 4.0), (w - 8.0, 4.0), 2, 0.0);
    for i in 1..=8 {
        let y = 4.0 + (h - 8.0) * i as f32 / 8.0;
        let _ = write!(d, " L{:.1} {y:.1}", w - 8.0 + if i % 2 == 1 { 6.0 } else { -2.0 } + rng.j(1.5));
    }
    d += &torn(rng, (w - 8.0, h - 4.0), (8.0, h - 4.0), 2, 0.0);
    for i in 1..=8 {
        let y = h - 4.0 - (h - 8.0) * i as f32 / 8.0;
        let _ = write!(d, " L{:.1} {y:.1}", 8.0 + if i % 2 == 1 { -6.0 } else { 2.0 } + rng.j(1.5));
    }
    d += " Z";
    let pat = match pattern {
        "rayas" => format!("<pattern id=\"p\" width=\"22\" height=\"22\" patternUnits=\"userSpaceOnUse\" patternTransform=\"rotate(35)\"><rect width=\"9\" height=\"22\" fill=\"{dark}\" opacity=\"0.35\"/></pattern>"),
        "lunares" => format!("<pattern id=\"p\" width=\"24\" height=\"24\" patternUnits=\"userSpaceOnUse\"><circle cx=\"6\" cy=\"6\" r=\"3.5\" fill=\"{dark}\" opacity=\"0.5\"/><circle cx=\"18\" cy=\"18\" r=\"3.5\" fill=\"{dark}\" opacity=\"0.5\"/></pattern>"),
        "cuadros" => format!("<pattern id=\"p\" width=\"20\" height=\"20\" patternUnits=\"userSpaceOnUse\"><rect width=\"10\" height=\"20\" fill=\"{dark}\" opacity=\"0.18\"/><rect width=\"20\" height=\"10\" fill=\"{dark}\" opacity=\"0.18\"/></pattern>"),
        "corazones" => format!("<pattern id=\"p\" width=\"30\" height=\"28\" patternUnits=\"userSpaceOnUse\"><path d=\"M15 21 C5 13 7 6 12 6 C14 6 15 8 15 9 C15 8 16 6 18 6 C23 6 25 13 15 21Z\" fill=\"{dark}\" opacity=\"0.45\"/></pattern>"),
        _ => String::new(),
    };
    let overlay = if pat.is_empty() { String::new() } else { format!("<path d=\"{d}\" fill=\"url(#p)\"/>") };
    svg(w, h, &pat, &format!("<g opacity=\"0.88\" filter=\"url(#pp)\"><path d=\"{d}\" fill=\"{fill}\"/>{overlay}</g>"))
}

fn masking_tape(rng: &mut Rng) -> String {
    let (w, h) = (260.0, 70.0);
    let mut d = String::from("M6 6");
    d += &torn(rng, (6.0, 6.0), (w - 6.0, 6.0), 3, 1.0);
    d += &torn(rng, (w - 6.0, 6.0), (w - 6.0, h - 6.0), 9, 5.0);
    d += &torn(rng, (w - 6.0, h - 6.0), (6.0, h - 6.0), 3, 1.0);
    d += &torn(rng, (6.0, h - 6.0), (6.0, 6.0), 9, 5.0);
    svg(w, h, "", &format!("<path d=\"{d} Z\" fill=\"#efe3c4\" opacity=\"0.85\" filter=\"url(#pp)\"/>"))
}

// ----- Papeles --------------------------------------------------------------

fn sticky_note(rng: &mut Rng, fill: &str) -> String {
    let s = 240.0;
    let curl = format!("M{s} {} Q{} {} {} {s} L{s} {s} Z", s - 34.0, s - 6.0, s - 6.0, s - 34.0);
    let _ = rng.next();
    svg(
        s + 10.0,
        s + 14.0,
        "<linearGradient id=\"c\" x1=\"0\" y1=\"0\" x2=\"1\" y2=\"1\"><stop offset=\"0\" stop-color=\"#000\" stop-opacity=\"0.18\"/><stop offset=\"1\" stop-color=\"#000\" stop-opacity=\"0\"/></linearGradient>",
        &format!(
            "<g filter=\"url(#pp)\"><path d=\"M5 5 H{s} V{c} Q{q} {q} {c} {s} H5 Z\" fill=\"{fill}\"/><path d=\"{curl}\" fill=\"url(#c)\" transform=\"translate(-0 0)\"/><rect x=\"5\" y=\"5\" width=\"{s0}\" height=\"34\" fill=\"#000\" opacity=\"0.04\"/></g>",
            c = s - 34.0,
            q = s - 8.0,
            s0 = s - 5.0
        ),
    )
}

fn torn_paper(rng: &mut Rng, fill: &str) -> String {
    let (w, h) = (300.0, 170.0);
    let mut d = String::from("M8 10");
    d += &torn(rng, (8.0, 10.0), (w - 8.0, 8.0), 24, 4.0);
    d += &torn(rng, (w - 8.0, 8.0), (w - 10.0, h - 10.0), 3, 1.0);
    d += &torn(rng, (w - 10.0, h - 10.0), (8.0, h - 8.0), 26, 5.0);
    svg(w, h, "", &format!("<path d=\"{d} Z\" fill=\"{fill}\" filter=\"url(#pp)\"/>"))
}

fn notebook_paper(rng: &mut Rng, grid: bool) -> String {
    let (w, h) = (240.0, 300.0);
    let mut d = String::from("M10 10");
    d += &torn(rng, (10.0, 10.0), (w - 10.0, 10.0), 2, 0.0);
    d += &torn(rng, (w - 10.0, 10.0), (w - 10.0, h - 10.0), 2, 0.0);
    d += &torn(rng, (w - 10.0, h - 10.0), (10.0, h - 10.0), 20, 4.0);
    let mut lines = String::new();
    if grid {
        let mut x = 22.0;
        while x < w - 10.0 {
            let _ = write!(lines, "<line x1=\"{x}\" y1=\"10\" x2=\"{x}\" y2=\"{}\" stroke=\"#9cc3e6\" stroke-width=\"1\"/>", h - 14.0);
            x += 16.0;
        }
        let mut y = 22.0;
        while y < h - 14.0 {
            let _ = write!(lines, "<line x1=\"10\" y1=\"{y}\" x2=\"{}\" y2=\"{y}\" stroke=\"#9cc3e6\" stroke-width=\"1\"/>", w - 10.0);
            y += 16.0;
        }
    } else {
        let mut y = 52.0;
        while y < h - 18.0 {
            let _ = write!(lines, "<line x1=\"10\" y1=\"{y}\" x2=\"{}\" y2=\"{y}\" stroke=\"#9cc3e6\" stroke-width=\"1.5\"/>", w - 10.0);
            y += 22.0;
        }
        lines += "<line x1=\"46\" y1=\"10\" x2=\"46\" y2=\"290\" stroke=\"#e88a8a\" stroke-width=\"1.5\"/>";
        for y in [60.0, 150.0, 240.0] {
            let _ = write!(lines, "<circle cx=\"24\" cy=\"{y}\" r=\"7\" fill=\"#d8d2c4\"/>");
        }
    }
    svg(w, h, "<clipPath id=\"k\"><rect x=\"10\" y=\"10\" width=\"220\" height=\"290\"/></clipPath>", &format!(
        "<g filter=\"url(#pp)\"><path d=\"{d} Z\" fill=\"#fdfcf7\"/><g clip-path=\"url(#k)\">{lines}</g></g>"
    ))
}

// ----- Etiquetas ------------------------------------------------------------

fn hang_tag(fill: &str, dark: &str) -> String {
    svg(200.0, 300.0, "", &format!(concat!(
        "<path d=\"M100 18 C70 -10 40 30 70 60\" fill=\"none\" stroke=\"#8a6238\" stroke-width=\"3\"/>",
        "<g filter=\"url(#pp)\"><path d=\"M60 40 L140 40 L180 90 L180 280 L20 280 L20 90 Z\" fill=\"{fill}\"/>",
        "<circle cx=\"100\" cy=\"85\" r=\"12\" fill=\"#fff\"/><circle cx=\"100\" cy=\"85\" r=\"12\" fill=\"none\" stroke=\"{dark}\" stroke-width=\"3\"/>",
        "<path d=\"M40 130 H160 M40 160 H140\" stroke=\"{dark}\" stroke-width=\"3\" stroke-dasharray=\"2 7\" stroke-linecap=\"round\" opacity=\"0.6\"/></g>"
    ), fill = fill, dark = dark))
}

fn ticket(fill: &str, dark: &str) -> String {
    svg(320.0, 150.0, "", &format!(concat!(
        "<g filter=\"url(#pp)\"><path d=\"M10 10 H310 V55 A20 20 0 0 0 310 95 V140 H10 V95 A20 20 0 0 0 10 55 Z\" fill=\"{fill}\"/>",
        "<line x1=\"235\" y1=\"18\" x2=\"235\" y2=\"132\" stroke=\"{dark}\" stroke-width=\"3\" stroke-dasharray=\"6 6\"/>",
        "<rect x=\"30\" y=\"30\" width=\"185\" height=\"90\" rx=\"6\" fill=\"none\" stroke=\"{dark}\" stroke-width=\"2.5\" opacity=\"0.7\"/>",
        "<path d=\"M262 40 L262 110\" stroke=\"{dark}\" stroke-width=\"10\" stroke-dasharray=\"3 4\" opacity=\"0.55\"/></g>"
    ), fill = fill, dark = dark))
}

fn scalloped_badge(fill: &str, dark: &str) -> String {
    let mut d = String::new();
    let n = 18;
    for i in 0..n {
        let a = std::f32::consts::TAU * i as f32 / n as f32;
        let b = std::f32::consts::TAU * (i as f32 + 0.5) / n as f32;
        let (x, y) = (120.0 + 108.0 * a.cos(), 120.0 + 108.0 * a.sin());
        let (cx, cy) = (120.0 + 124.0 * b.cos(), 120.0 + 124.0 * b.sin());
        if i == 0 {
            let _ = write!(d, "M{x:.1} {y:.1}");
        }
        let a2 = std::f32::consts::TAU * (i + 1) as f32 / n as f32;
        let _ = write!(d, " Q{cx:.1} {cy:.1} {:.1} {:.1}", 120.0 + 108.0 * a2.cos(), 120.0 + 108.0 * a2.sin());
    }
    svg(240.0, 246.0, "", &format!(
        "<g filter=\"url(#pp)\"><path d=\"{d} Z\" fill=\"{fill}\"/><circle cx=\"120\" cy=\"120\" r=\"80\" fill=\"none\" stroke=\"{dark}\" stroke-width=\"3\" stroke-dasharray=\"3 6\" stroke-linecap=\"round\"/></g>"
    ))
}

fn bunting(rng: &mut Rng) -> String {
    let mut body = String::from("<path d=\"M10 20 Q200 60 390 20\" fill=\"none\" stroke=\"#8a6238\" stroke-width=\"3\"/>");
    for i in 0..6 {
        let t = (i as f32 + 0.5) / 6.0;
        let x = 10.0 + 380.0 * t;
        let y = 20.0 + 160.0 * t * (1.0 - t) * 0.5 * 2.0 * 0.5;
        let (_, fill, _) = PALETTE[(i * 3 + 1) % PALETTE.len()];
        let _ = write!(body, "<path d=\"M{:.1} {:.1} L{:.1} {:.1} L{:.1} {:.1} Z\" fill=\"{fill}\" filter=\"url(#pp)\"/>", x - 28.0, y + 2.0, x + 28.0, y + 2.0, x + rng.j(3.0), y + 78.0);
    }
    svg(400.0, 130.0, "", &body)
}

fn ribbon_banner(fill: &str, dark: &str) -> String {
    svg(380.0, 120.0, "", &format!(concat!(
        "<g filter=\"url(#pp)\"><path d=\"M10 40 H70 V100 H10 L30 70 Z\" fill=\"{dark}\"/><path d=\"M370 40 H310 V100 H370 L350 70 Z\" fill=\"{dark}\"/>",
        "<path d=\"M50 20 Q190 0 330 20 V80 Q190 60 50 80 Z\" fill=\"{fill}\"/></g>"
    ), fill = fill, dark = dark))
}

fn label(fill: &str, dark: &str) -> String {
    svg(320.0, 120.0, "", &format!(concat!(
        "<g filter=\"url(#pp)\"><path d=\"M30 10 H290 L310 30 V90 L290 110 H30 L10 90 V30 Z\" fill=\"{fill}\"/>",
        "<path d=\"M38 22 H282 L296 36 V84 L282 98 H38 L24 84 V36 Z\" fill=\"none\" stroke=\"{dark}\" stroke-width=\"2.5\"/></g>"
    ), fill = fill, dark = dark))
}

// ----- Clips ----------------------------------------------------------------

fn paperclip(color: &str) -> String {
    svg(90.0, 230.0, "", &format!(
        "<path d=\"M30 60 V170 A15 15 0 0 0 60 170 V40 A25 25 0 0 0 10 40 V180 A35 35 0 0 0 80 180 V70\" fill=\"none\" stroke=\"{color}\" stroke-width=\"7\" stroke-linecap=\"round\" filter=\"url(#sh)\"/>"
    ))
}

fn push_pin(fill: &str, dark: &str) -> String {
    svg(120.0, 150.0, "<radialGradient id=\"g\" cx=\"0.35\" cy=\"0.3\" r=\"0.7\"><stop offset=\"0\" stop-color=\"#fff\" stop-opacity=\"0.7\"/><stop offset=\"1\" stop-color=\"#fff\" stop-opacity=\"0\"/></radialGradient>", &format!(concat!(
        "<g filter=\"url(#sh)\"><line x1=\"60\" y1=\"90\" x2=\"60\" y2=\"140\" stroke=\"#9a9a9a\" stroke-width=\"4\" stroke-linecap=\"round\"/>",
        "<ellipse cx=\"60\" cy=\"88\" rx=\"34\" ry=\"10\" fill=\"{dark}\"/><path d=\"M42 40 H78 L72 85 H48 Z\" fill=\"{fill}\"/>",
        "<ellipse cx=\"60\" cy=\"40\" rx=\"38\" ry=\"24\" fill=\"{fill}\"/><ellipse cx=\"60\" cy=\"40\" rx=\"38\" ry=\"24\" fill=\"url(#g)\"/></g>"
    ), fill = fill, dark = dark))
}

fn clothespin() -> String {
    svg(90.0, 260.0, "", concat!(
        "<g filter=\"url(#sh)\"><rect x=\"14\" y=\"10\" width=\"30\" height=\"240\" rx=\"8\" fill=\"#e2b884\"/>",
        "<rect x=\"46\" y=\"10\" width=\"30\" height=\"240\" rx=\"8\" fill=\"#d6a76f\"/>",
        "<rect x=\"8\" y=\"118\" width=\"74\" height=\"34\" rx=\"10\" fill=\"none\" stroke=\"#a8a8a8\" stroke-width=\"6\"/></g>"
    ))
}

fn binder_clip(color: &str) -> String {
    svg(160.0, 170.0, "", &format!(concat!(
        "<g filter=\"url(#sh)\"><path d=\"M50 88 L38 14 H70 L60 88 M100 88 L90 14 H122 L110 88\" fill=\"none\" stroke=\"#b8b8b8\" stroke-width=\"6\" stroke-linejoin=\"round\"/>",
        "<path d=\"M20 80 Q80 64 140 80 L130 160 H30 Z\" fill=\"{color}\"/><path d=\"M20 80 Q80 64 140 80 L137 98 Q80 84 23 98 Z\" fill=\"#fff\" opacity=\"0.18\"/></g>"
    ), color = color))
}

// ----- Garabatos ------------------------------------------------------------

fn doodle(rng: &mut Rng, kind: &str, color: &str) -> String {
    let w = 6.0;
    let wob = 2.5;
    match kind {
        "corazon" => {
            let pts: Vec<(f32, f32)> = (0..=40)
                .map(|i| {
                    let t = std::f32::consts::TAU * i as f32 / 40.0;
                    let x = 16.0 * t.sin().powi(3);
                    let y = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
                    (110.0 + x * 5.6, 100.0 - y * 5.6)
                })
                .collect();
            svg(220.0, 200.0, "", &ink(&hand(rng, &pts, wob), color, w))
        }
        "estrella" => {
            let pts: Vec<(f32, f32)> = (0..=10)
                .map(|i| {
                    let r = if i % 2 == 0 { 90.0 } else { 38.0 };
                    let a = std::f32::consts::PI * i as f32 / 5.0 - std::f32::consts::FRAC_PI_2;
                    (110.0 + r * a.cos(), 105.0 + r * a.sin())
                })
                .collect();
            let mut d = String::new();
            for (i, (x, y)) in pts.iter().enumerate() {
                let _ = write!(d, "{}{:.1} {:.1} ", if i == 0 { "M" } else { "L" }, x + rng.j(wob), y + rng.j(wob));
            }
            svg(220.0, 210.0, "", &ink(&d, color, w))
        }
        "flecha" => {
            let body = format!("M20 145 C{:.1} {:.1} {:.1} {:.1} 248 92", 50.0 + rng.j(8.0), 40.0 + rng.j(8.0), 170.0 + rng.j(8.0), 20.0 + rng.j(8.0));
            let head = "M214 70 L250 92 L212 110";
            svg(280.0, 170.0, "", &(ink(&body, color, w) + &ink(head, color, w)))
        }
        "espiral" => {
            let pts: Vec<(f32, f32)> = (0..=60)
                .map(|i| {
                    let t = i as f32 / 60.0 * std::f32::consts::TAU * 3.0;
                    let r = 6.0 + t * 4.6;
                    (110.0 + r * t.cos(), 110.0 + r * t.sin())
                })
                .collect();
            svg(220.0, 220.0, "", &ink(&hand(rng, &pts, 1.0), color, w))
        }
        "subrayado" => {
            let pts: Vec<(f32, f32)> = (0..=12).map(|i| (15.0 + i as f32 * 24.0, 40.0 + if i % 2 == 0 { -10.0 } else { 10.0 })).collect();
            svg(320.0, 80.0, "", &ink(&hand(rng, &pts, 1.5), color, w))
        }
        "circulo" => {
            let pts: Vec<(f32, f32)> = (0..=46)
                .map(|i| {
                    let t = std::f32::consts::TAU * i as f32 / 40.0;
                    let r = 1.0 + 0.04 * (i as f32 / 46.0);
                    (150.0 + 130.0 * r * t.cos(), 90.0 + 70.0 * r * t.sin())
                })
                .collect();
            svg(300.0, 180.0, "", &ink(&hand(rng, &pts, 2.0), color, w))
        }
        "destellos" => {
            let star = |cx: f32, cy: f32, r: f32| {
                format!(
                    "M{cx} {} Q{cx} {cy} {} {cy} Q{cx} {cy} {cx} {} Q{cx} {cy} {} {cy} Q{cx} {cy} {cx} {} Z",
                    cy - r,
                    cx + r,
                    cy + r,
                    cx - r,
                    cy - r
                )
            };
            let body = format!(
                "<path d=\"{} {} {}\" fill=\"{color}\"/>",
                star(80.0, 90.0, 60.0),
                star(165.0, 45.0, 30.0),
                star(160.0, 150.0, 22.0)
            );
            svg(210.0, 190.0, "", &body)
        }
        "nube" => {
            let j = |r: &mut Rng| r.j(3.0);
            let d = format!(
                "M48 140 A32 32 0 0 1 {:.1} 86 A44 44 0 0 1 {:.1} 52 A38 38 0 0 1 {:.1} 76 A32 32 0 0 1 212 140 Z",
                52.0 + j(rng),
                124.0 + j(rng),
                190.0 + j(rng)
            );
            svg(260.0, 170.0, "", &ink(&d, color, w))
        }
        "rayo" => {
            let mut d = String::new();
            for (i, (x, y)) in [(120.0, 10.0), (50.0, 120.0), (105.0, 120.0), (80.0, 220.0), (170.0, 90.0), (115.0, 90.0), (150.0, 10.0)].iter().enumerate() {
                let _ = write!(d, "{}{:.1} {:.1} ", if i == 0 { "M" } else { "L" }, x + rng.j(wob), y + rng.j(wob));
            }
            svg(220.0, 230.0, "", &format!("<path d=\"{d}Z\" fill=\"{color}\" fill-opacity=\"0.25\" stroke=\"{color}\" stroke-width=\"{w}\" stroke-linejoin=\"round\"/>"))
        }
        "sonrisa" => {
            let face: Vec<(f32, f32)> = (0..=40).map(|i| {
                let t = std::f32::consts::TAU * i as f32 / 40.0;
                (110.0 + 90.0 * t.cos(), 110.0 + 90.0 * t.sin())
            }).collect();
            let smile = hand(rng, &[(65.0, 125.0), (90.0, 155.0), (130.0, 155.0), (155.0, 125.0)], 1.5);
            let eyes = format!("<circle cx=\"80\" cy=\"88\" r=\"8\" fill=\"{color}\"/><circle cx=\"140\" cy=\"88\" r=\"8\" fill=\"{color}\"/>");
            svg(220.0, 220.0, "", &(ink(&hand(rng, &face, wob), color, w) + &ink(&smile, color, w) + &eyes))
        }
        _ /* "x" */ => {
            let mut body = String::new();
            for i in 0..3 {
                let x = 40.0 + i as f32 * 80.0;
                let d = format!("M{} 20 L{} 80 M{} 20 L{} 80", x - 25.0 + rng.j(3.0), x + 25.0, x + 25.0 + rng.j(3.0), x - 25.0);
                body += &ink(&d, color, w);
            }
            svg(240.0, 100.0, "", &body)
        }
    }
}

// ----- Naturaleza -----------------------------------------------------------

fn flower(rng: &mut Rng, petal: &str, center: &str, petals: usize) -> String {
    let mut body = String::new();
    for i in 0..petals {
        let a = 360.0 * i as f32 / petals as f32 + rng.j(4.0);
        let _ = write!(body, "<ellipse cx=\"110\" cy=\"58\" rx=\"{}\" ry=\"48\" fill=\"{petal}\" transform=\"rotate({a:.1} 110 110)\"/>", if petals > 6 { 18.0 } else { 30.0 });
    }
    let _ = write!(body, "<circle cx=\"110\" cy=\"110\" r=\"26\" fill=\"{center}\"/>");
    svg(220.0, 224.0, "", &format!("<g filter=\"url(#pp)\">{body}</g>"))
}

fn leaf(rng: &mut Rng, fill: &str, dark: &str) -> String {
    let vein = hand(rng, &[(40.0, 230.0), (90.0, 160.0), (140.0, 80.0), (175.0, 30.0)], 1.0);
    svg(220.0, 250.0, "", &format!(
        "<g filter=\"url(#pp)\"><path d=\"M40 230 C10 130 90 30 180 25 C200 120 130 210 40 230 Z\" fill=\"{fill}\"/>{}</g>",
        ink(&vein, dark, 4.0)
    ))
}

fn sprig(rng: &mut Rng, color: &str) -> String {
    let stem = hand(rng, &[(30.0, 260.0), (70.0, 180.0), (100.0, 100.0), (120.0, 20.0)], 1.5);
    let mut leaves = String::new();
    for (i, (x, y)) in [(62.0, 200.0), (80.0, 150.0), (96.0, 105.0), (108.0, 62.0)].iter().enumerate() {
        let side = if i % 2 == 0 { -1.0 } else { 1.0 };
        let _ = write!(leaves, "<ellipse cx=\"{}\" cy=\"{y}\" rx=\"26\" ry=\"11\" fill=\"{color}\" transform=\"rotate({} {} {y})\"/>", x + side * 24.0, side * -30.0, x + side * 24.0);
    }
    svg(180.0, 270.0, "", &format!("{}<g filter=\"url(#pp)\">{leaves}</g>", ink(&stem, color, 5.0)))
}

fn sun(rng: &mut Rng) -> String {
    let mut rays = String::new();
    for i in 0..12 {
        let a = std::f32::consts::TAU * i as f32 / 12.0;
        let d = format!("M{:.1} {:.1} L{:.1} {:.1}", 120.0 + 70.0 * a.cos(), 120.0 + 70.0 * a.sin(), 120.0 + (100.0 + rng.j(6.0)) * a.cos(), 120.0 + (100.0 + rng.j(6.0)) * a.sin());
        rays += &ink(&d, "#e8a93a", 7.0);
    }
    svg(240.0, 240.0, "", &format!("{rays}<circle cx=\"120\" cy=\"120\" r=\"56\" fill=\"#f6d36b\" filter=\"url(#pp)\"/>"))
}

fn rainbow() -> String {
    let mut body = String::new();
    for (i, c) in ["#f5a3a3", "#f8c8a4", "#f6df86", "#b6e0c6", "#a8d3ef", "#cdb9ec"].iter().enumerate() {
        let r = 130.0 - i as f32 * 17.0;
        let _ = write!(body, "<path d=\"M{} 150 A{r} {r} 0 0 1 {} 150\" fill=\"none\" stroke=\"{c}\" stroke-width=\"17\"/>", 150.0 - r, 150.0 + r);
    }
    svg(300.0, 160.0, "", &format!("<g filter=\"url(#pp)\">{body}</g>"))
}

fn moon() -> String {
    svg(200.0, 210.0, "", "<path d=\"M130 20 A90 90 0 1 0 180 160 A75 75 0 1 1 130 20 Z\" fill=\"#f6df86\" filter=\"url(#pp)\"/>")
}

// ----- Sellos ---------------------------------------------------------------

fn postage_stamp(fill: &str, dark: &str, motif: &str) -> String {
    let (w, h) = (200.0, 240.0);
    let mut d = String::from("M0 0");
    let r = 7.0;
    let mut x = 0.0;
    while x < w {
        let _ = write!(d, " H{:.1} A{r} {r} 0 0 0 {:.1} 0", x + 6.0, x + 6.0 + 2.0 * r);
        x += 6.0 + 2.0 * r;
    }
    let _ = write!(d, " H{w} V{h} H0 Z");
    let inner = match motif {
        "corazon" => format!("<path d=\"M100 175 C40 130 50 80 80 80 C92 80 100 90 100 98 C100 90 108 80 120 80 C150 80 160 130 100 175 Z\" fill=\"{dark}\"/>"),
        "estrella" => format!("<path d=\"M100 70 L114 112 L158 112 L122 138 L136 180 L100 154 L64 180 L78 138 L42 112 L86 112 Z\" fill=\"{dark}\"/>"),
        _ => format!("<path d=\"M40 175 L80 110 L105 145 L125 120 L160 175 Z\" fill=\"{dark}\"/><circle cx=\"140\" cy=\"85\" r=\"15\" fill=\"{dark}\"/>"),
    };
    svg(w + 12.0, h + 14.0, "", &format!(concat!(
        "<g transform=\"translate(6 6)\" filter=\"url(#pp)\"><path d=\"{d}\" fill=\"#fffdf6\"/>",
        "<rect x=\"18\" y=\"22\" width=\"164\" height=\"196\" fill=\"{fill}\"/>{inner}</g>"
    ), d = d, fill = fill, inner = inner))
}

fn postmark(color: &str) -> String {
    let mut waves = String::new();
    for i in 0..5 {
        let y = 70.0 + i as f32 * 18.0;
        let _ = write!(waves, "<path d=\"M150 {y} Q175 {} 200 {y} T250 {y} T300 {y}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"4\"/>", y - 9.0);
    }
    svg(310.0, 220.0, "", &format!(concat!(
        "<g opacity=\"0.8\"><circle cx=\"100\" cy=\"110\" r=\"85\" fill=\"none\" stroke=\"{color}\" stroke-width=\"5\"/>",
        "<circle cx=\"100\" cy=\"110\" r=\"62\" fill=\"none\" stroke=\"{color}\" stroke-width=\"3\"/>",
        "<path d=\"M60 110 H140\" stroke=\"{color}\" stroke-width=\"4\"/>{waves}</g>"
    ), color = color, waves = waves))
}

// ---------------------------------------------------------------------------

pub fn stickers() -> &'static [Sticker] {
    static LIST: OnceLock<Vec<Sticker>> = OnceLock::new();
    LIST.get_or_init(build)
}

fn build() -> Vec<Sticker> {
    let mut out = Vec::new();
    let mut add = |id: String, name: String, category: &'static str, keywords: &str, svg: String| {
        let search = fold(&format!("{name} {category} {keywords}"));
        out.push(Sticker { id, name, category, search, svg });
    };
    let rng = |id: &str| Rng::new(id);

    // Cintas
    for (pattern, label) in [("lisa", "lisa"), ("rayas", "de rayas"), ("lunares", "de lunares"), ("cuadros", "de cuadros"), ("corazones", "de corazones")] {
        for &(color, fill, dark) in PALETTE.iter().take(6) {
            let id = format!("cinta-{pattern}-{color}");
            let svg = washi(&mut rng(&id), fill, dark, pattern);
            add(id, format!("Cinta washi {label} {color}"), "Cintas", "washi tape cinta adhesiva decorativa", svg);
        }
    }
    add("cinta-masking".into(), "Cinta de papel".into(), "Cintas", "masking tape adhesiva beige pegar", masking_tape(&mut rng("masking")));
    add("cinta-masking-2".into(), "Cinta de papel corta".into(), "Cintas", "masking tape adhesiva beige pegar", masking_tape(&mut rng("masking-2")));

    // Papeles
    for &(color, fill, _) in PALETTE.iter().take(6) {
        let id = format!("nota-{color}");
        add(id.clone(), format!("Nota adhesiva {color}"), "Papeles", "post-it nota recordatorio papel", sticky_note(&mut rng(&id), fill));
    }
    for (color, fill) in [("blanco", "#fdfcf7"), ("kraft", "#cfa77c"), ("rosa", "#f5b3c6"), ("celeste", "#a8d3ef")] {
        let id = format!("papel-rasgado-{color}");
        add(id.clone(), format!("Papel rasgado {color}"), "Papeles", "roto rasgado trozo papel collage", torn_paper(&mut rng(&id), fill));
    }
    add("hoja-rayada".into(), "Hoja de cuaderno rayada".into(), "Papeles", "cuaderno libreta escuela notas rayas", notebook_paper(&mut rng("rayada"), false));
    add("hoja-cuadriculada".into(), "Hoja cuadriculada".into(), "Papeles", "cuaderno cuadricula matematicas notas", notebook_paper(&mut rng("cuadros"), true));

    // Etiquetas
    for &(color, fill, dark) in PALETTE.iter().step_by(2) {
        add(format!("etiqueta-colgante-{color}"), format!("Etiqueta colgante {color}"), "Etiquetas", "tag precio regalo cordel", hang_tag(fill, dark));
        add(format!("boleto-{color}"), format!("Boleto {color}"), "Etiquetas", "ticket entrada cine concierto viaje", ticket(fill, dark));
        add(format!("insignia-{color}"), format!("Insignia festoneada {color}"), "Etiquetas", "badge circulo sello festoneado", scalloped_badge(fill, dark));
        add(format!("cartel-{color}"), format!("Cinta de título {color}"), "Etiquetas", "banner listón titulo encabezado", ribbon_banner(fill, dark));
        add(format!("rotulo-{color}"), format!("Rótulo {color}"), "Etiquetas", "label etiqueta nombre marco", label(fill, dark));
    }
    add("banderines".into(), "Banderines de fiesta".into(), "Etiquetas", "guirnalda fiesta cumpleaños celebracion bunting", bunting(&mut rng("banderines")));

    // Clips
    for (color, c) in [("plateado", "#9ea3a8"), ("dorado", "#c9a24a"), ("rosa", "#e07a9a"), ("azul", "#5b8fd1")] {
        add(format!("clip-{color}"), format!("Clip {color}"), "Clips", "sujetapapeles clip oficina metal", paperclip(c));
    }
    for &(color, fill, dark) in PALETTE.iter().take(6) {
        add(format!("chincheta-{color}"), format!("Chincheta {color}"), "Clips", "chinche alfiler tablero pin corcho", push_pin(fill, dark));
    }
    add("pinza".into(), "Pinza de madera".into(), "Clips", "pinza ropa madera colgar foto", clothespin());
    for (color, c) in [("negro", "#2f2f2f"), ("rosa", "#e07a9a")] {
        add(format!("clip-mariposa-{color}"), format!("Clip mariposa {color}"), "Clips", "binder pinza oficina sujetar", binder_clip(c));
    }

    // Garabatos
    let doodles = [
        ("corazon", "Corazón dibujado", "amor love corazon garabato"),
        ("estrella", "Estrella dibujada", "star favorito brillo garabato"),
        ("flecha", "Flecha curva", "arrow señalar apuntar garabato"),
        ("espiral", "Espiral", "remolino garabato hipnosis"),
        ("subrayado", "Subrayado en zigzag", "linea subrayar resaltar garabato"),
        ("circulo", "Círculo marcado", "rodear resaltar marcar garabato"),
        ("destellos", "Destellos", "brillo sparkle magia estrellas"),
        ("nube", "Nube dibujada", "cielo clima garabato"),
        ("rayo", "Rayo", "energia tormenta electricidad"),
        ("sonrisa", "Carita sonriente", "feliz smile cara garabato"),
        ("x", "Equis", "besos tachar marcas xoxo"),
    ];
    for (kind, name, kw) in doodles {
        for (color, c) in [("negro", "#2b2b2b"), ("rosa", "#e05a8a"), ("azul", "#3f6fc4")] {
            let id = format!("garabato-{kind}-{color}");
            let svg = doodle(&mut rng(&id), kind, c);
            add(id, format!("{name} {color}"), "Garabatos", kw, svg);
        }
    }

    // Naturaleza
    for &(color, fill, dark) in PALETTE.iter().take(6) {
        let id = format!("flor-{color}");
        add(id.clone(), format!("Flor {color}"), "Naturaleza", "flor primavera petalos jardin", flower(&mut rng(&id), fill, dark, 5));
    }
    add("margarita".into(), "Margarita".into(), "Naturaleza", "flor blanca primavera campo", flower(&mut rng("margarita"), "#fffdf6", "#f2c94c", 12));
    add("hoja-verde".into(), "Hoja verde".into(), "Naturaleza", "planta hoja natural verde", leaf(&mut rng("hoja"), "#9fd3a7", "#4f9a6c"));
    add("hoja-otono".into(), "Hoja de otoño".into(), "Naturaleza", "otoño hoja naranja", leaf(&mut rng("otono"), "#f2a65a", "#b5602a"));
    add("ramita".into(), "Ramita".into(), "Naturaleza", "rama hojas planta botanica", sprig(&mut rng("ramita"), "#6aa86a"));
    add("sol".into(), "Sol".into(), "Naturaleza", "sol verano dia calor", sun(&mut rng("sol")));
    add("luna".into(), "Luna".into(), "Naturaleza", "luna noche dormir", moon());
    add("arcoiris".into(), "Arcoíris".into(), "Naturaleza", "arcoiris colores lluvia orgullo", rainbow());

    // Sellos
    for (i, &(color, fill, dark)) in PALETTE.iter().take(6).enumerate() {
        let motif = ["corazon", "estrella", "montañas"][i % 3];
        add(format!("sello-{motif}-{color}"), format!("Sello postal {motif} {color}"), "Sellos", "estampilla correo carta viaje", postage_stamp(fill, dark, motif));
    }
    for (color, c) in [("negro", "#3a3a3a"), ("rojo", "#c0504d"), ("azul", "#3f6fc4")] {
        add(format!("matasellos-{color}"), format!("Matasellos {color}"), "Sellos", "correo carta postal tinta", postmark(c));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::{rasterize, sanitize_svg, svg_aspect};

    #[test]
    fn coleccion_completa_y_dibujable() {
        let list = stickers();
        assert!(list.len() >= 100, "{}", list.len());
        let mut ids = std::collections::HashSet::new();
        for s in list {
            assert!(ids.insert(&s.id), "id repetido {}", s.id);
            assert!(CATEGORIES.contains(&s.category));
            let clean = sanitize_svg(&s.svg).unwrap_or_else(|| panic!("{} no es un SVG válido", s.id));
            let a = svg_aspect(&clean);
            let (w, h) = ((96.0 * a.min(1.0)).max(8.0), (96.0 / a.max(1.0)).max(8.0));
            let kind = crate::doc::Kind::Graphic { svg: clean, source: s.id.clone(), name: s.name.clone(), tint: None, sticker: false };
            let svg = crate::vector::layer_svg(&kind, w, h).unwrap();
            let img = rasterize(&svg, 1.0, (0.0, 0.0), (w as u32, h as u32)).unwrap();
            assert!(img.pixels().filter(|p| p[3] > 100).count() > 50, "{} sale vacío", s.id);
        }
        for c in CATEGORIES {
            assert!(list.iter().any(|s| s.category == c), "falta {c}");
        }
    }

    /// Hoja de contactos de toda la colección, para revisarla a ojo:
    /// `CARRUSEL_TEST_OUT=carpeta cargo test hoja_de_stickers -- --ignored`
    #[test]
    #[ignore]
    fn hoja_de_stickers() {
        let out = std::env::var("CARRUSEL_TEST_OUT").map(std::path::PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
        let (cell, cols) = (150u32, 12u32);
        let list = stickers();
        let rows = (list.len() as u32).div_ceil(cols);
        let mut sheet = image::RgbaImage::from_pixel(cell * cols, cell * rows, image::Rgba([246, 241, 232, 255]));
        for (i, s) in list.iter().enumerate() {
            let a = svg_aspect(&s.svg);
            let side = cell as f32 - 20.0;
            let (w, h) = if a >= 1.0 { (side, side / a) } else { (side * a, side) };
            let kind = crate::doc::Kind::Graphic { svg: s.svg.clone(), source: String::new(), name: String::new(), tint: None, sticker: false };
            let img = rasterize(&crate::vector::layer_svg(&kind, w, h).unwrap(), 1.0, (0.0, 0.0), (w as u32, h as u32)).unwrap();
            let (x, y) = ((i as u32 % cols) * cell, (i as u32 / cols) * cell);
            image::imageops::overlay(&mut sheet, &img, (x + (cell - w as u32) / 2) as i64, (y + (cell - h as u32) / 2) as i64);
        }
        sheet.save(out.join("stickers.png")).unwrap();
    }

    #[test]
    fn busca_en_espanol() {
        let hits = |q: &str| stickers().iter().filter(|s| s.search.contains(&fold(q))).count();
        assert!(hits("cinta") >= 30);
        assert!(hits("corazón") >= 3);
        assert!(hits("chincheta") >= 6);
    }
}
