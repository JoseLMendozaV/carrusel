# Carrusel

Creador de carruseles panorámicos para Instagram al estilo de SCRL, escrito en Rust con [egui](https://github.com/emilk/egui). Corre nativo en macOS (Metal), y también en Windows y Linux.

Trabajas sobre **un lienzo continuo** dividido en slides. Lo que cruce la línea entre dos slides se ve como un deslizamiento sin cortes cuando publicas el carrusel. Al exportar, cada slide sale como su propio archivo (`slide_01.jpg`, `slide_02.jpg`…) en el orden de publicación.

## Qué hace

- Formatos 4:5 (1080×1350), 1:1, 3:4 e historia 9:16; **slides ilimitados** (Instagram publica hasta 20 por post: pasado ese número la app te avisa con ⚠, pero no te limita).
- Fotos: arrastrar a la ventana o «＋ Fotos». Respeta la orientación EXIF del iPhone. JPG, PNG, WebP, GIF, BMP, TIFF.
- Mover, redimensionar desde la esquina rosa, imán a bordes y centros de cada slide y de las demás capas (⌥ para desactivarlo).
- Encajar una foto en 1 o varias slides (rellenar o encajar completa), o como panorámica de todo el carrusel. «Rellenar» recorta la foto justo al área elegida, sin invadir los slides vecinos.
- **Recortar** arrastrando los tiradores blancos de los lados (izquierda, derecha, arriba, abajo): la foto no se mueve ni se deforma, solo se oculta lo que queda fuera, y puedes volver a alargarla hasta su borde original. Mientras recortas ves la foto entera en transparente, y el borde se pega a las líneas entre slides. **⌘ + arrastrar** mueve la foto dentro del recorte.
- «Distribuir fotos: 1 por slide».
- Textos multilínea con tamaño, color, alineación, recuadro de fondo y sombra (más estilos listos en la biblioteca); puedes elegir cualquier fuente `.ttf`/`.otf`/`.ttc` de tu Mac.
- **Biblioteca** (panel izquierdo):
  - **Emojis y calcomanías**: 1.869 emojis en **dos estilos**, Twemoji (plano) y **Google Noto** (con degradados y banderas ondeadas), todos en SVG y nítidos a cualquier tamaño. Buscador en español («corazón», «fuego», «gato»…) y 9 categorías. Con «Como calcomanía» llevan borde blanco y sombra; el estilo de un emoji se puede cambiar después en el inspector.
  - **Stickers scrapbook** (colección propia de Carrusel, 145 diseños): cintas washi (lisas, rayas, lunares, cuadros, corazones), papeles (notas adhesivas, papel rasgado, hojas de cuaderno), etiquetas (colgantes, boletos, insignias, cintas de título, banderines), clips y chinchetas, garabatos a mano, flores y hojas, y sellos postales. Con buscador en español y categorías.
  - **Iconos** de [Iconify](https://iconify.design): unos 475 000 iconos de código abierto, con búsqueda (también en español) y color a elegir. Solo se muestran colecciones con licencias aptas para publicar (se excluyen las no comerciales y GPL); el inspector indica la licencia de cada icono.
  - **Ilustraciones** de [FreeSVG](https://freesvg.org): imágenes decorativas de dominio público (CC0). Su catálogo exige una cuenta gratuita: entras una vez en la pestaña y Carrusel guarda solo la sesión, nunca la contraseña.
  - Iconos, ilustraciones y stickers se guardan dentro del proyecto, así que se abren y exportan sin conexión, y también admiten el efecto calcomanía.
  - **Formas**: rectángulo, redondeado, círculo, triángulo, estrella, corazón, flecha, línea, bocadillo y estallido, con relleno y borde.
  - **Marcos**: simple, doble línea, Polaroid, película, esquinas y discontinuo. Con una foto seleccionada, el marco se ajusta a ella.
  - **Textos**: título, subtítulo, párrafo, etiqueta, resaltado y texto sobre foto.
- **Videos** (MP4, MOV, M4V, AVI, MKV, WebM, WMV, FLV, 3GP, MPG, TS…): se reproducen en el editor, se recortan y encajan como las fotos y puedes elegir en qué segundo empiezan. Respeta la rotación de los videos del iPhone.
- Marco de color en fotos y videos.
- Esquinas redondeadas en las fotos y opacidad en cualquier capa.
- Clic derecho sobre el lienzo: insertar un slide antes o después, eliminar un slide (lo que sigue se recoloca) y acciones de la capa.
- **Vista previa** tipo Instagram: un slide a la vez, deslizando con el ratón o con ← →, con puntos de paginación y contador.
- **Panel de Capas** (abajo a la derecha, como en Photoshop): todas las capas de la que está al frente a la del fondo, con miniatura y nombre. Clic para seleccionar, **arrastrar para cambiar el orden**, doble clic para renombrar, **ojo** para ocultar (no se ve ni se exporta) y **candado** para bloquear (no se puede seleccionar ni mover desde el lienzo, ideal para fondos). Botones para subir, bajar, duplicar, ocultar, bloquear y eliminar, y la opción «Solo este slide» para cuando hay muchos objetos.
- Color de fondo, orden de capas, duplicar, deshacer/rehacer (un arrastre de deslizador o una edición de texto cuentan como un solo paso).
- Avisa antes de cerrar, abrir otro proyecto o empezar uno nuevo si hay cambios sin guardar.
- Proyectos guardables (`.carrusel`, JSON con rutas a tus fotos).
- Exporta JPG (calidad 95) o PNG a resolución completa, más el panorama entero opcional. Cada slide se dibuja por separado, así la memoria no crece con el número de slides, y la barra de estado muestra el progreso. Con 100 o más slides los nombres llevan más ceros (`slide_001.jpg`) para que se ordenen bien; un panorama de más de 65 535 px de ancho se guarda en PNG porque JPG no lo admite. La exportación usa las fotos originales, no las miniaturas, y solo escala la parte visible de cada foto.
- **Exportar para Instagram y TikTok**:
  - **Carrusel**: un archivo por slide; los slides con video salen como **MP4** (H.264 + AAC, 30 fps, hasta 60 s, con el sonido del video) y el resto como foto.
  - **Video 9:16** (1080×1920, para Reels y TikTok): un solo MP4 que recorre el carrusel deslizándose de slide en slide; eliges cuántos segundos se queda cada uno.

## Requisitos

Instala Rust si no lo tienes:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Para los videos hace falta [ffmpeg](https://ffmpeg.org) (sin él, todo lo demás funciona igual):

```sh
brew install ffmpeg
```

## Ejecutar

```sh
cargo run --release
cargo test        # pruebas de exportación y del modelo
```

## Crear la app `Carrusel.app`

```sh
chmod +x build_mac.sh
./build_mac.sh            # para tu Mac
./build_mac.sh universal  # Apple Silicon + Intel
```

La app queda en `dist/Carrusel.app`. Para ponerle icono, deja un `icon.png` de 1024×1024 junto al script antes de compilar.

## Atajos

| Atajo | Acción |
| --- | --- |
| ⌘N | Nuevo proyecto |
| ⌘O / ⌘S / ⇧⌘S | Abrir / guardar / guardar como |
| ⌘E | Exportar slides |
| ⌘Z / ⇧⌘Z | Deshacer / rehacer |
| ⌘D | Duplicar |
| ⌫ | Eliminar |
| Flechas (⇧ ×10) | Mover 1 px |
| ⌘ + rueda o pellizco | Zoom |
| Esc | Quitar selección / salir de la vista previa |
| P | Vista previa tipo Instagram (← → para pasar de slide) |
| Clic derecho | Insertar / eliminar slides, acciones de la capa |
| Lados blancos | Recortar la foto |
| ⌘ + arrastrar | Mover la foto dentro de su recorte |

## Estructura

- `src/doc.rs` — modelo (formato, slides, capas) y cálculo de encaje.
- `src/render.rs` — carga de imágenes, texto con `ab_glyph` y exportación.
- `src/vector.rs` — emojis, formas y marcos como SVG (dibujados con resvg).
- `src/video.rs` — videos con ffmpeg: información, reproducción y codificación MP4.
- `src/library.rs` — panel de la biblioteca.
- `src/layers.rs` — panel de capas (orden, visibilidad, bloqueo, nombres).
- `src/scrapbook.rs` — colección propia de stickers scrapbook (SVG generados).
- `src/online.rs` — Iconify y FreeSVG (búsqueda, descarga y caché en `~/Library/Caches/Carrusel`).
- `src/main.rs` — interfaz, lienzo e interacción.
- `assets/emoji.tsv` y `assets/emoji_noto.zlib` — biblioteca de emojis (Twemoji y Noto), generada con `tools/build_emoji.py`.

## Ideas para seguir

Plantillas de cuadrícula/collage, rotación, contorno en el texto, música de fondo para el video 9:16.

## Créditos

- Emojis: [Twemoji](https://github.com/jdecked/twemoji) © Twitter, Inc. y colaboradores, licencia [CC-BY 4.0](https://creativecommons.org/licenses/by/4.0/).
- Emojis: [Google Noto Emoji](https://github.com/googlefonts/noto-emoji) © Google, licencia [Apache 2.0](https://www.apache.org/licenses/LICENSE-2.0); banderas de [region-flags](https://github.com/googlefonts/noto-emoji/tree/main/third_party/region-flags), de dominio público.
- Iconos: [Iconify](https://iconify.design); cada colección tiene su propia licencia, indicada en el inspector.
- Ilustraciones: [FreeSVG.org](https://freesvg.org), dominio público (CC0).
- Nombres de emojis en español: [Unicode CLDR](https://cldr.unicode.org), licencia [Unicode](https://www.unicode.org/license.txt).
