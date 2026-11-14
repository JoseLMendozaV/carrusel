// Puente entre Carrusel (Rust/WebAssembly) y el navegador.
//
// Las funciones que reciben bytes desde Rust (`&[u8]`) obtienen una vista de
// la memoria de WebAssembly que puede cambiar en cuanto Rust vuelve a pedir
// memoria: por eso se copian en cuanto llegan si se van a usar más tarde.

// ---------------------------------------------------------------------------
// Archivos

/// Abre el selector de archivos. Hay que llamarla durante un toque o clic
/// (Safari no deja abrirlo en otro momento). Devuelve [{name, bytes}].
export function pick_files(accept, multiple) {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    if (accept) input.accept = accept;
    input.multiple = multiple;
    input.style.cssText = "position:fixed;left:-1000px;top:0;opacity:0";
    document.body.appendChild(input);
    let settled = false;
    const finish = (list) => {
      if (settled) return;
      settled = true;
      input.remove();
      resolve(list);
    };
    input.addEventListener("change", async () => {
      const out = [];
      for (const f of Array.from(input.files || [])) {
        try {
          out.push({ name: f.name, bytes: new Uint8Array(await f.arrayBuffer()) });
        } catch (e) {
          console.warn("No se pudo leer", f.name, e);
        }
      }
      finish(out);
    });
    input.addEventListener("cancel", () => finish([]));
    input.click();
  });
}

/// Descarga un archivo (en iPad va a Archivos › Descargas).
export function download(name, bytes, mime) {
  const blob = new Blob([new Uint8Array(bytes)], { type: mime });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.style.display = "none";
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

/// ¿Se pueden compartir archivos (hoja de compartir de iOS: «Guardar imágenes»)?
export function can_share_files() {
  try {
    const f = new File([new Uint8Array([0])], "x.jpg", { type: "image/jpeg" });
    return !!(navigator.canShare && navigator.canShare({ files: [f] }));
  } catch (_) {
    return false;
  }
}

/// Abre la hoja de compartir con los archivos [{name, mime, bytes}].
/// Hay que llamarla durante un toque. Resuelve "ok", "cancel" o un error.
export function share_files(list, title) {
  try {
    const files = list.map((f) => new File([f.bytes], f.name, { type: f.mime }));
    return navigator.share({ files, title }).then(
      () => "ok",
      (e) => (e && e.name === "AbortError" ? "cancel" : "error: " + (e && e.message ? e.message : e)),
    );
  } catch (e) {
    return Promise.resolve("error: " + (e && e.message ? e.message : e));
  }
}

export function confirm_dialog(message) {
  return window.confirm(message);
}

// ---------------------------------------------------------------------------
// Imágenes: el navegador las decodifica (más rápido que WebAssembly, fuera del
// hilo principal, y entiende HEIC en Safari). Respeta la orientación EXIF.

export async function decode_image(bytes, maxPixels, maxSide) {
  const data = new Uint8Array(bytes);
  try {
    const bmp = await createImageBitmap(new Blob([data]));
    const s = Math.min(1, maxSide / Math.max(bmp.width, bmp.height), Math.sqrt(maxPixels / (bmp.width * bmp.height)));
    const w = Math.max(1, Math.round(bmp.width * s));
    const h = Math.max(1, Math.round(bmp.height * s));
    let canvas;
    if (typeof OffscreenCanvas !== "undefined") {
      canvas = new OffscreenCanvas(w, h);
    } else {
      canvas = document.createElement("canvas");
      canvas.width = w;
      canvas.height = h;
    }
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(bmp, 0, 0, w, h);
    if (bmp.close) bmp.close();
    const pixels = ctx.getImageData(0, 0, w, h).data;
    // Libera la memoria del lienzo cuanto antes (Safari limita el total).
    canvas.width = 1;
    canvas.height = 1;
    return { width: w, height: h, pixels: new Uint8Array(pixels.buffer) };
  } catch (e) {
    console.warn("El navegador no pudo decodificar la imagen:", e);
    return null;
  }
}

// ---------------------------------------------------------------------------
// Red

/// `headers` es [nombre, valor, nombre, valor…]. Devuelve los bytes de la
/// respuesta o lanza un mensaje de error.
export async function http(method, url, headers, body) {
  const h = new Headers();
  for (let i = 0; i + 1 < headers.length; i += 2) h.append(headers[i], headers[i + 1]);
  let resp;
  try {
    resp = await fetch(url, { method, headers: h, body: body == null ? undefined : body });
  } catch (e) {
    throw "Sin conexión o el servidor no responde.";
  }
  const bytes = new Uint8Array(await resp.arrayBuffer());
  if (!resp.ok) {
    const text = new TextDecoder().decode(bytes).slice(0, 200);
    throw `Error ${resp.status} del servidor. ${text}`;
  }
  return bytes;
}

export function sleep_ms(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

// ---------------------------------------------------------------------------
// Guardado automático en IndexedDB (el proyecto y sus fotos)

let dbPromise = null;

function db() {
  if (!dbPromise) {
    dbPromise = new Promise((resolve, reject) => {
      const req = indexedDB.open("carrusel", 1);
      req.onupgradeneeded = () => req.result.createObjectStore("kv");
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
  }
  return dbPromise;
}

function run(mode, fn) {
  return db().then(
    (d) =>
      new Promise((resolve, reject) => {
        const t = d.transaction("kv", mode);
        const req = fn(t.objectStore("kv"));
        t.oncomplete = () => resolve(req ? req.result : undefined);
        t.onerror = () => reject(t.error);
        t.onabort = () => reject(t.error);
      }),
  );
}

/// Devuelve el valor guardado (texto o Uint8Array) o null.
export function idb_get(key) {
  return run("readonly", (s) => s.get(key)).then(
    (v) => (v === undefined ? null : v),
    () => null,
  );
}

export function idb_set_text(key, text) {
  return run("readwrite", (s) => s.put(text, key)).then(() => true, () => false);
}

export function idb_set_bytes(key, bytes) {
  const copy = new Uint8Array(bytes);
  return run("readwrite", (s) => s.put(copy, key)).then(() => true, () => false);
}

export function idb_delete(key) {
  return run("readwrite", (s) => s.delete(key)).then(() => true, () => false);
}

export function idb_keys() {
  return run("readonly", (s) => s.getAllKeys()).then(
    (k) => k || [],
    () => [],
  );
}

/// Pide al navegador que no borre los datos guardados por falta de espacio.
export function request_persistence() {
  try {
    if (navigator.storage && navigator.storage.persist) navigator.storage.persist();
  } catch (_) {}
}
