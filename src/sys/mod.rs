//! Lo que cambia entre la app de escritorio y la versión web: tareas en
//! segundo plano, lectura de archivos, red y ajustes guardados.

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_arch = "wasm32")]
pub use web::*;

/// `true` en la versión web (iPad y navegadores).
pub const WEB: bool = cfg!(target_arch = "wasm32");

/// En escritorio las tareas corren en otro hilo y deben ser `Send`; en la
/// web corren en el mismo hilo (sin esa exigencia).
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> MaybeSend for T {}
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

/// Una petición HTTP.
pub struct Request<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub headers: Vec<(&'a str, String)>,
    pub body: Option<String>,
}

impl<'a> Request<'a> {
    pub fn get(url: &'a str) -> Self {
        Self { method: "GET", url, headers: Vec::new(), body: None }
    }

    pub fn header(mut self, name: &'a str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}
