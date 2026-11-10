#!/usr/bin/env python3
"""Genera assets/emoji.tsv: la biblioteca de emojis/calcomanías de Carrusel.

Combina tres fuentes abiertas:
  - Twemoji 15 (SVG, CC-BY 4.0):  https://registry.npmjs.org/@twemoji/svg/-/svg-15.0.0.tgz
  - Lista y categorías de Unicode: https://unicode.org/Public/emoji/latest/emoji-test.txt
  - Nombres y palabras clave en español (Unicode CLDR):
      cldr-json/cldr-annotations-full/annotations/es/annotations.json
      cldr-json/cldr-annotations-derived-full/annotationsDerived/es/annotations.json

Opcional, segundo estilo: Google Noto Emoji 2D (SVG, Apache 2.0; banderas de
dominio público) → assets/emoji_noto.zlib (TSV «emoji  SVG» comprimido con zlib):
  https://github.com/googlefonts/noto-emoji  (2D/svg y third_party/region-flags/waved-svg)

Uso:  python3 tools/build_emoji.py <carpeta_svg> <emoji-test.txt> <es.json> <es_derived.json> [carpeta_noto]

Formato de salida (una línea por emoji, separada por tabuladores):
  emoji  categoría  nombre  palabras clave (separadas por |)  SVG en una línea
Se omiten los tonos de piel para que la biblioteca no se llene de variantes.
"""
import json
import os
import re
import sys
import zlib

GROUPS = {
    "Smileys & Emotion": "Caras",
    "People & Body": "Personas",
    "Animals & Nature": "Naturaleza",
    "Food & Drink": "Comida",
    "Travel & Places": "Viajes",
    "Activities": "Actividades",
    "Objects": "Objetos",
    "Symbols": "Símbolos",
    "Flags": "Banderas",
}
SKIN_TONES = {0x1F3FB, 0x1F3FC, 0x1F3FD, 0x1F3FE, 0x1F3FF}


def twemoji_name(cps):
    # Regla de Twemoji: sin FE0F salvo que la secuencia lleve ZWJ (200D).
    if 0x200D not in cps:
        cps = [c for c in cps if c != 0xFE0F]
    return "-".join(f"{c:x}" for c in cps)


def noto_name(emoji):
    return "emoji_u" + "_".join(f"{ord(c):04x}" for c in emoji if ord(c) != 0xFE0F) + ".svg"


def minify(svg):
    """Deja el SVG listo para anidarlo: sin cabeceras ni comentarios, con las
    entidades de Illustrator sustituidas y sin tamaño ni posición en la raíz
    (Carrusel los pone al dibujarlo)."""
    entities = dict(re.findall(r'<!ENTITY\s+(\w+)\s+"([^"]*)"', svg))
    svg = re.sub(r"<\?xml[^>]*\?>", "", svg)
    svg = re.sub(r"<!DOCTYPE.*?\]>|<!DOCTYPE[^>]*>", "", svg, flags=re.S)
    svg = re.sub(r"<!--.*?-->", "", svg, flags=re.S)
    for name, value in entities.items():
        svg = svg.replace(f"&{name};", value)
    # Datos privados de Illustrator (pesados e inútiles para dibujar).
    svg = re.sub(r"<i:pgf\b.*?</i:pgf>", "", svg, flags=re.S)
    svg = re.sub(r"<metadata\b.*?</metadata>", "", svg, flags=re.S)
    svg = re.sub(r">\s+<", "><", svg)
    svg = " ".join(svg.split())
    start = svg.find("<svg")
    end = svg.find(">", start)
    root = re.sub(r'\s(?:x|y|width|height)="[^"]*"', "", svg[start:end])
    return root + svg[end:]


def read_noto(noto_dir, name, depth=0):
    """Lee un SVG de Noto; algunas banderas son enlaces (symlinks de git) a otra."""
    path = os.path.join(noto_dir, name)
    if not os.path.exists(path):
        return None
    with open(path, encoding="utf-8", errors="replace") as f:
        text = f.read()
    if "<svg" not in text and depth < 3:
        return read_noto(noto_dir, os.path.basename(text.strip()), depth + 1)
    return text if "<svg" in text else None


def annotations(path, key):
    with open(path, encoding="utf-8") as f:
        return json.load(f)[key]["annotations"]


def main():
    svg_dir, test_txt, es_path, es_derived_path = sys.argv[1:5]
    names = annotations(es_path, "annotations")
    names.update(annotations(es_derived_path, "annotationsDerived"))

    out = []
    group = None
    with open(test_txt, encoding="utf-8") as f:
        for line in f:
            if line.startswith("# group:"):
                group = GROUPS.get(line.split(":", 1)[1].strip())
                continue
            if not group or line.startswith("#") or "; fully-qualified" not in line:
                continue
            cps = [int(c, 16) for c in line.split(";")[0].split()]
            if SKIN_TONES & set(cps):
                continue
            emoji = "".join(chr(c) for c in cps)
            svg_path = os.path.join(svg_dir, twemoji_name(cps) + ".svg")
            if not os.path.exists(svg_path):
                continue
            with open(svg_path, encoding="utf-8") as s:
                svg = minify(s.read())
            # CLDR guarda los nombres sin FE0F.
            ann = names.get(emoji) or names.get(emoji.replace("️", "")) or {}
            english = line.split("#", 1)[1].split(" ", 3)[-1].strip()
            name = (ann.get("tts") or [english])[0]
            keywords = "|".join(k for k in ann.get("default", []) if "\t" not in k)
            out.append("\t".join([emoji, group, name, keywords, svg]))

    os.makedirs("assets", exist_ok=True)
    with open("assets/emoji.tsv", "w", encoding="utf-8") as f:
        f.write("\n".join(out) + "\n")
    print(f"{len(out)} emojis → assets/emoji.tsv")

    if len(sys.argv) > 5:
        noto_dir = sys.argv[5]
        rows = []
        for line in out:
            emoji = line.split("\t", 1)[0]
            text = read_noto(noto_dir, noto_name(emoji))
            if text:
                rows.append(emoji + "\t" + minify(text))
        data = ("\n".join(rows) + "\n").encode("utf-8")
        with open("assets/emoji_noto.zlib", "wb") as f:
            f.write(zlib.compress(data, 9))
        print(f"{len(rows)} emojis de Noto → assets/emoji_noto.zlib "
              f"({len(data) / 1e6:.1f} MB → {os.path.getsize('assets/emoji_noto.zlib') / 1e6:.1f} MB)")


if __name__ == "__main__":
    main()
