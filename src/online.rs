//! Bibliotecas en línea: iconos de Iconify y, con tu cuenta, ilustraciones de
//! FreeSVG. En escritorio usa `curl` y guarda en disco lo descargado; en la
//! web usa `fetch` (ver `sys`).

use crate::sys::{self, Request};
use crate::vector::fold;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

pub async fn get(url: &str) -> Result<Vec<u8>, String> {
    sys::http(Request::get(url)).await
}

/// Descarga con caché (los iconos no cambian).
pub(crate) async fn cached_get(key: &str, url: &str, max_age_days: Option<u64>) -> Result<Vec<u8>, String> {
    if let Some(data) = sys::cache_get(key, max_age_days) {
        return Ok(data);
    }
    let data = get(url).await?;
    sys::cache_put(key, &data);
    Ok(data)
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Español → inglés (Iconify y FreeSVG solo entienden búsquedas en inglés)

const DICTIONARY: &[(&str, &str)] = &[
    ("corazon", "heart"), ("estrella", "star"), ("casa", "home"), ("hogar", "home"), ("flecha", "arrow"),
    ("flor", "flower"), ("flores", "flower"), ("hoja", "leaf"), ("arbol", "tree"), ("sol", "sun"), ("luna", "moon"),
    ("nube", "cloud"), ("lluvia", "rain"), ("nieve", "snow"), ("fuego", "fire"), ("agua", "water"), ("mar", "sea"),
    ("playa", "beach"), ("montana", "mountain"), ("cielo", "sky"), ("gato", "cat"), ("perro", "dog"), ("pajaro", "bird"),
    ("ave", "bird"), ("pez", "fish"), ("mariposa", "butterfly"), ("abeja", "bee"), ("caballo", "horse"), ("conejo", "rabbit"),
    ("oso", "bear"), ("leon", "lion"), ("comida", "food"), ("cafe", "coffee"), ("te", "tea"), ("pastel", "cake"),
    ("torta", "cake"), ("pizza", "pizza"), ("fruta", "fruit"), ("manzana", "apple"), ("helado", "ice cream"), ("vino", "wine"),
    ("cerveza", "beer"), ("musica", "music"), ("nota", "note"), ("libro", "book"), ("lapiz", "pencil"), ("pluma", "pen"),
    ("camara", "camera"), ("foto", "photo"), ("video", "video"), ("telefono", "phone"), ("movil", "phone"), ("celular", "phone"),
    ("correo", "mail"), ("carta", "letter"), ("mensaje", "message"), ("chat", "chat"), ("usuario", "user"), ("persona", "person"),
    ("gente", "people"), ("nino", "child"), ("bebe", "baby"), ("mujer", "woman"), ("hombre", "man"), ("familia", "family"),
    ("amor", "love"), ("feliz", "happy"), ("triste", "sad"), ("sonrisa", "smile"), ("cara", "face"), ("mano", "hand"),
    ("ojo", "eye"), ("buscar", "search"), ("lupa", "search"), ("ajustes", "settings"), ("configuracion", "settings"),
    ("calendario", "calendar"), ("reloj", "clock"), ("hora", "time"), ("tiempo", "time"), ("dinero", "money"), ("tienda", "shop"),
    ("compra", "shopping"), ("carrito", "cart"), ("bolsa", "bag"), ("regalo", "gift"), ("fiesta", "party"), ("cumpleanos", "birthday"),
    ("globo", "balloon"), ("navidad", "christmas"), ("halloween", "halloween"), ("viaje", "travel"), ("avion", "plane"),
    ("coche", "car"), ("carro", "car"), ("auto", "car"), ("bicicleta", "bicycle"), ("tren", "train"), ("barco", "boat"),
    ("mapa", "map"), ("ubicacion", "location"), ("lugar", "place"), ("mundo", "world"), ("tierra", "earth"), ("planeta", "planet"),
    ("candado", "lock"), ("llave", "key"), ("seguridad", "security"), ("escudo", "shield"), ("campana", "bell"), ("alerta", "alert"),
    ("aviso", "warning"), ("informacion", "info"), ("ayuda", "help"), ("pregunta", "question"), ("comprobar", "check"),
    ("visto", "check"), ("cerrar", "close"), ("borrar", "delete"), ("basura", "trash"), ("editar", "edit"), ("guardar", "save"),
    ("descargar", "download"), ("subir", "upload"), ("compartir", "share"), ("enlace", "link"), ("nube", "cloud"),
    ("archivo", "file"), ("carpeta", "folder"), ("documento", "document"), ("imagen", "image"), ("estrella", "star"),
    ("trofeo", "trophy"), ("medalla", "medal"), ("corona", "crown"), ("diamante", "diamond"), ("joya", "jewel"),
    ("deporte", "sport"), ("futbol", "soccer"), ("pelota", "ball"), ("juego", "game"), ("ejercicio", "fitness"),
    ("salud", "health"), ("medico", "medical"), ("hospital", "hospital"), ("escuela", "school"), ("trabajo", "work"),
    ("oficina", "office"), ("empresa", "business"), ("grafica", "chart"), ("grafico", "chart"), ("estadistica", "statistics"),
    ("idea", "idea"), ("bombilla", "lightbulb"), ("foco", "lightbulb"), ("rayo", "lightning"), ("energia", "energy"),
    ("planta", "plant"), ("cactus", "cactus"), ("hojas", "leaves"), ("otono", "autumn"), ("primavera", "spring"),
    ("verano", "summer"), ("invierno", "winter"), ("marco", "frame"), ("borde", "border"), ("adorno", "ornament"),
    ("ornamento", "ornament"), ("decoracion", "decoration"), ("decorativo", "decorative"), ("vintage", "vintage"),
    ("retro", "retro"), ("floral", "floral"), ("cinta", "ribbon"), ("lazo", "bow"), ("banderin", "banner"), ("bandera", "flag"),
    ("etiqueta", "label"), ("sello", "stamp"), ("silueta", "silhouette"), ("patron", "pattern"), ("textura", "texture"),
    ("acuarela", "watercolor"), ("dibujo", "drawing"), ("garabato", "doodle"), ("mandala", "mandala"), ("calavera", "skull"),
    ("unicornio", "unicorn"), ("dinosaurio", "dinosaur"), ("robot", "robot"), ("cohete", "rocket"), ("espacio", "space"),
    ("instagram", "instagram"), ("redes", "social"), ("me gusta", "like"), ("pulgar", "thumb"), ("burbuja", "bubble"),
    ("ola", "wave"), ("paisaje", "landscape"), ("ciudad", "city"), ("edificio", "building"), ("iglesia", "church"),
];

/// Traduce las palabras que conoce y deja las demás tal cual.
pub fn to_english(query: &str) -> String {
    let folded = fold(query.trim());
    if let Some((_, en)) = DICTIONARY.iter().find(|(es, _)| *es == folded) {
        return en.to_string();
    }
    folded
        .split_whitespace()
        .map(|w| {
            let singular = w.strip_suffix("es").filter(|s| s.len() > 2).or_else(|| w.strip_suffix('s')).unwrap_or(w);
            DICTIONARY
                .iter()
                .find(|(es, _)| *es == w || *es == singular)
                .map(|(_, en)| en.to_string())
                .unwrap_or_else(|| w.to_string())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------
// Iconify (https://iconify.design): ~475 000 iconos de código abierto

#[derive(Clone, Debug)]
pub struct IconSet {
    pub name: String,
    pub license: String,
    pub spdx: String,
}

/// Licencias que permiten usar los iconos en publicaciones (también comerciales).
fn license_ok(spdx: &str) -> bool {
    !spdx.contains("NC") && !spdx.contains("ND") && !spdx.starts_with("GPL") && !spdx.is_empty()
}

static SETS: OnceLock<HashMap<String, IconSet>> = OnceLock::new();

/// Información de una colección ya cargada (sin conectarse a internet).
pub fn icon_set(prefix: &str) -> Option<&'static IconSet> {
    SETS.get()?.get(prefix)
}

/// Colecciones de Iconify con licencia apta (se refresca cada semana).
pub async fn icon_sets() -> Result<&'static HashMap<String, IconSet>, String> {
    if let Some(s) = SETS.get() {
        return Ok(s);
    }
    #[derive(Deserialize)]
    struct License {
        #[serde(default)]
        title: String,
        #[serde(default)]
        spdx: String,
    }
    #[derive(Deserialize)]
    struct Info {
        name: String,
        license: Option<License>,
        #[serde(default)]
        hidden: bool,
    }
    let data = cached_get("iconify/collections.json", "https://api.iconify.design/collections", Some(7)).await?;
    let raw: HashMap<String, Info> = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    let sets = raw
        .into_iter()
        .filter(|(_, i)| !i.hidden)
        .filter_map(|(prefix, i)| {
            let l = i.license?;
            license_ok(&l.spdx).then_some((prefix, IconSet { name: i.name, license: l.title, spdx: l.spdx }))
        })
        .collect();
    Ok(SETS.get_or_init(|| sets))
}

/// Busca iconos; devuelve nombres «prefijo:icono» y si hay más resultados.
pub async fn search_icons(query: &str, start: usize) -> Result<(Vec<String>, bool), String> {
    const LIMIT: usize = 96;
    #[derive(Deserialize)]
    struct Res {
        icons: Vec<String>,
        total: usize,
    }
    let sets = icon_sets().await?;
    let q = to_english(query);
    // La API pagina así: `limit` es el total acumulado y `start` el primero a devolver.
    let url = format!("https://api.iconify.design/search?query={}&limit={}&start={start}", encode(&q), start + LIMIT);
    let res: Res = serde_json::from_slice(&get(&url).await?).map_err(|e| e.to_string())?;
    let more = res.total > start + res.icons.len();
    let icons: Vec<String> =
        res.icons.into_iter().filter(|n| n.split_once(':').is_some_and(|(p, _)| sets.contains_key(p))).collect();
    Ok((icons, more))
}

/// SVG de varios iconos de golpe (agrupados por colección), con caché.
pub async fn icon_svgs(names: &[String]) -> HashMap<String, String> {
    #[derive(Deserialize)]
    struct Icon {
        body: String,
        width: Option<f32>,
        height: Option<f32>,
        left: Option<f32>,
        top: Option<f32>,
    }
    #[derive(Deserialize)]
    struct Set {
        #[serde(default)]
        icons: HashMap<String, Icon>,
        width: Option<f32>,
        height: Option<f32>,
    }
    let mut out = HashMap::new();
    let mut by_prefix: HashMap<&str, Vec<&str>> = HashMap::new();
    for n in names {
        let key = format!("iconify/icons/{}.svg", n.replace(':', "_"));
        if let Some(svg) = sys::cache_get(&key, None).and_then(|b| String::from_utf8(b).ok()) {
            out.insert(n.clone(), svg);
        } else if let Some((p, i)) = n.split_once(':') {
            by_prefix.entry(p).or_default().push(i);
        }
    }
    for (prefix, icons) in by_prefix {
        let url = format!("https://api.iconify.design/{prefix}.json?icons={}", icons.join(","));
        let Ok(data) = get(&url).await else { continue };
        let Ok(set) = serde_json::from_slice::<Set>(&data) else { continue };
        for (name, icon) in set.icons {
            let w = icon.width.or(set.width).unwrap_or(16.0);
            let h = icon.height.or(set.height).unwrap_or(16.0);
            let svg = format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" viewBox=\"{} {} {w} {h}\">{}</svg>",
                icon.left.unwrap_or(0.0),
                icon.top.unwrap_or(0.0),
                icon.body
            );
            let full = format!("{prefix}:{name}");
            sys::cache_put(&format!("iconify/icons/{prefix}_{name}.svg"), svg.as_bytes());
            out.insert(full, svg);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// FreeSVG (https://freesvg.org): ilustraciones de dominio público (CC0).
// Su API exige una cuenta gratuita: se guarda solo el token, nunca la contraseña.

const FREESVG_API: &str = "https://freesvg.org/api/v1";

const TOKEN_KEY: &str = "freesvg_token";

pub fn freesvg_token() -> Option<String> {
    sys::setting_get(TOKEN_KEY)
}

pub fn freesvg_logout() {
    sys::setting_remove(TOKEN_KEY);
}

pub async fn freesvg_login(email: &str, password: &str) -> Result<String, String> {
    let body = serde_json::json!({ "email": email, "password": password }).to_string();
    let url = format!("{FREESVG_API}/auth/login");
    let mut req = Request::get(&url).header("Accept", "application/json").header("Content-Type", "application/json");
    req.method = "POST";
    req.body = Some(body);
    let data = sys::http(req).await.map_err(|_| "No se pudo iniciar sesión: revisa el correo y la contraseña.".to_string())?;
    let json: serde_json::Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    let token = json["token"]
        .as_str()
        .or(json["access_token"].as_str())
        .ok_or_else(|| json["message"].as_str().unwrap_or("FreeSVG no devolvió un token.").to_string())?
        .to_string();
    sys::setting_set(TOKEN_KEY, &token)?;
    Ok(token)
}

#[derive(Clone, Debug)]
pub struct FreeSvgItem {
    pub id: u64,
    pub title: String,
    pub thumb: String,
    pub svg: String,
}

async fn freesvg_request(token: &str, url: &str, accept: &str) -> Result<Vec<u8>, String> {
    let req = Request::get(url).header("Accept", accept).header("Authorization", format!("Bearer {token}"));
    sys::http(req).await.map_err(|e| {
        if e.contains("401") || e.to_lowercase().contains("unauthenticated") {
            "La sesión de FreeSVG caducó: vuelve a entrar.".to_string()
        } else {
            e
        }
    })
}

/// Lee una página de resultados de la API de FreeSVG. Los campos `thumb` y
/// `svg` traen solo el nombre del archivo; las direcciones completas están en
/// `thumb_url` y `download_url`/`svg_url`.
fn parse_freesvg(json: &serde_json::Value, page: u32) -> (Vec<FreeSvgItem>, bool) {
    let items = json["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|it| {
                    let text = |k: &str| it[k].as_str().filter(|s| !s.is_empty()).map(str::to_string);
                    let id = it["id"].as_u64().or_else(|| it["id"].as_str().and_then(|s| s.parse().ok()))?;
                    let thumb = text("thumb_url")
                        .or_else(|| text("thumb").map(|t| format!("https://freesvg.org/storage/img/blog/{t}")))?;
                    let svg = text("download_url")
                        .or_else(|| text("svg_url"))
                        .unwrap_or_else(|| format!("{FREESVG_API}/svg/{id}/download"));
                    Some(FreeSvgItem { id, title: text("name").or_else(|| text("title")).unwrap_or_default(), thumb, svg })
                })
                .collect()
        })
        .unwrap_or_default();
    let more = json["meta"]["last_page"].as_u64().is_some_and(|last| (page as u64) < last);
    (items, more)
}

/// Busca ilustraciones (25 por página); devuelve resultados y si hay más páginas.
pub async fn freesvg_search(token: &str, query: &str, page: u32) -> Result<(Vec<FreeSvgItem>, bool), String> {
    let url = format!("{FREESVG_API}/search?query={}&page={page}", encode(&to_english(query)));
    let data = freesvg_request(token, &url, "application/json").await?;
    let json: serde_json::Value = serde_json::from_slice(&data).map_err(|_| "FreeSVG respondió algo inesperado.".to_string())?;
    Ok(parse_freesvg(&json, page))
}

/// Descarga el SVG de una ilustración (FreeSVG exige la sesión también aquí;
/// sin ella devuelve una página web con captcha en lugar del archivo).
pub async fn freesvg_download(token: &str, url: &str) -> Result<String, String> {
    let data = freesvg_request(token, url, "image/svg+xml, */*").await?;
    let text = String::from_utf8_lossy(&data).into_owned();
    if !text.contains("<svg") {
        return Err("FreeSVG no entregó el archivo: vuelve a entrar con tu cuenta.".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traduce_busquedas_comunes() {
        assert_eq!(to_english("Corazón"), "heart");
        assert_eq!(to_english("flores"), "flower");
        assert_eq!(to_english("gato feliz"), "cat happy");
        assert_eq!(to_english("vintage"), "vintage");
    }

    #[test]
    fn licencias_aptas() {
        assert!(license_ok("MIT") && license_ok("Apache-2.0") && license_ok("CC-BY-4.0") && license_ok("CC0-1.0"));
        assert!(!license_ok("CC-BY-NC-4.0") && !license_ok("GPL-3.0") && !license_ok(""));
    }

    #[test]
    fn lee_resultados_reales_de_freesvg() {
        // Fragmento de una respuesta real de /api/v1/search.
        let json: serde_json::Value = serde_json::from_str(r#"{"data":[{"id":110536,"name":"Flower - Christmas Flower","slug":"flower-christmas-flower",
            "thumb":"Flower-Christmas-Flower.png","thumb_url":"https:\/\/freesvg.org\/storage\/img\/blog\/Flower-Christmas-Flower.png",
            "svg":"Flower-Christmas-Flower.svg","svg_url":"https:\/\/freesvg.org\/api\/v1\/svg\/110536\/download",
            "download_url":"https:\/\/freesvg.org\/api\/v1\/svg\/110536\/download","license_title":"Public Domain"},
            {"id":2,"name":"","thumb":"b.png","svg":"b.svg"}],
            "meta":{"current_page":1,"last_page":40,"per_page":25,"total":999}}"#).unwrap();
        let (items, more) = parse_freesvg(&json, 1);
        assert!(more);
        assert_eq!(items[0].title, "Flower - Christmas Flower");
        assert_eq!(items[0].thumb, "https://freesvg.org/storage/img/blog/Flower-Christmas-Flower.png");
        assert_eq!(items[0].svg, "https://freesvg.org/api/v1/svg/110536/download");
        // Sin URLs completas, se construyen con las rutas conocidas.
        assert_eq!(items[1].thumb, "https://freesvg.org/storage/img/blog/b.png");
        assert_eq!(items[1].svg, "https://freesvg.org/api/v1/svg/2/download");
        assert!(!parse_freesvg(&json, 40).1, "la última página no tiene más");
    }

    #[test]
    fn codifica_la_busqueda() {
        assert_eq!(encode("ice cream&x"), "ice+cream%26x");
    }

    /// Necesita internet: `cargo test iconify -- --ignored`
    #[test]
    #[ignore]
    fn iconify_en_vivo() {
        let (icons, _) = pollster::block_on(search_icons("corazón", 0)).unwrap();
        assert!(!icons.is_empty());
        let svgs = pollster::block_on(icon_svgs(&icons[..5.min(icons.len())]));
        assert!(!svgs.is_empty());
        for svg in svgs.values() {
            assert!(crate::vector::sanitize_svg(svg).is_some(), "{svg}");
        }
    }

    /// Necesita internet y haber entrado en FreeSVG desde la app:
    /// `cargo test freesvg_en_vivo -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn freesvg_en_vivo() {
        let token = freesvg_token().expect("entra primero en FreeSVG desde la app");
        let (items, more) = pollster::block_on(freesvg_search(&token, "flores", 1)).unwrap();
        println!("{} resultados en la página 1 (más páginas: {more})", items.len());
        assert!(!items.is_empty());
        let mut drawn = 0;
        for item in items.iter().take(4) {
            let thumb = pollster::block_on(get(&item.thumb)).ok().and_then(|b| image::load_from_memory(&b).ok());
            let svg = pollster::block_on(freesvg_download(&token, &item.svg)).unwrap();
            let clean = crate::vector::sanitize_svg(&svg);
            println!("#{} «{}»: miniatura {}, SVG {} KB, dibujable {}", item.id, item.title, thumb.is_some(), svg.len() / 1024, clean.is_some());
            if let Some(clean) = clean {
                let kind = crate::doc::Kind::Graphic { svg: clean, source: String::new(), name: String::new(), tint: None, sticker: false };
                let img = crate::vector::rasterize(&crate::vector::layer_svg(&kind, 200.0, 200.0).unwrap(), 1.0, (0.0, 0.0), (200, 200)).unwrap();
                if img.pixels().any(|p| p[3] > 0) {
                    drawn += 1;
                }
            }
        }
        assert!(drawn > 0, "ninguna ilustración se pudo dibujar");
    }
}
