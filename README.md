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
- Textos multilínea con tamaño, color y alineación (izquierda, centro, derecha); puedes elegir cualquier fuente `.ttf`/`.otf`/`.ttc` de tu Mac.
- Esquinas redondeadas en las fotos y opacidad en cualquier capa.
- Clic derecho sobre el lienzo: insertar un slide antes o después, eliminar un slide (lo que sigue se recoloca) y acciones de la capa.
- **Vista previa** tipo Instagram: un slide a la vez, deslizando con el ratón o con ← →, con puntos de paginación y contador.
- Color de fondo, orden de capas, duplicar, deshacer/rehacer (un arrastre de deslizador o una edición de texto cuentan como un solo paso).
- Avisa antes de cerrar, abrir otro proyecto o empezar uno nuevo si hay cambios sin guardar.
- Proyectos guardables (`.carrusel`, JSON con rutas a tus fotos).
- Exporta JPG (calidad 95) o PNG a resolución completa, más el panorama entero opcional. Cada slide se dibuja por separado, así la memoria no crece con el número de slides, y la barra de estado muestra el progreso. Con 100 o más slides los nombres llevan más ceros (`slide_001.jpg`) para que se ordenen bien; un panorama de más de 65 535 px de ancho se guarda en PNG porque JPG no lo admite. La exportación usa las fotos originales, no las miniaturas, y solo escala la parte visible de cada foto.

## Requisitos

Instala Rust si no lo tienes:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
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
- `src/main.rs` — interfaz, lienzo e interacción.

## Ideas para seguir

Plantillas de cuadrícula/collage, rotación, sombras y contorno en el texto.
