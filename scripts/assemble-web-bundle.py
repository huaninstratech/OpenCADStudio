#!/usr/bin/env python3
"""Assemble a web bundle from a built wasm, without waiting for trunk.

`trunk build --release` spends ten to seventy minutes in wasm-bindgen on this
crate and then sometimes never finishes packaging, so a deploy is assembled
here instead: the wasm-bindgen output plus everything the template copies, with
the loader written inline.

    scripts/assemble-web-bundle.py <wasm> <out_dir> [--drop-cjk-fonts]

What it has to reproduce, and why each part matters:

* **The parse worker.** The page spawns `worker_pkg/ocs_web_worker.js` to read a
  drawing off the UI thread. Left out, every open shows
  "parsing entities... 10%" forever: the script 404s and its reply never comes.

* **The fonts.** `web/fonts/` is copied as a directory, not flattened into the
  bundle root, or every TTF the page asks for 404s.

* **The video thumbnails.** They are not in the repository: the Pages workflow
  downloads them from YouTube. A hand-made bundle has to do the same or the
  Start page's Videos tab shows blank cards.

* **The service worker's cache name.** `web/sw.js` ships with
  `CACHE_VERSION = "v1"` and expects the Pages workflow to stamp it. Served
  unstamped, every returning visitor keeps the first cache and is given an
  `index.html` naming assets the deploy has replaced.

* **The loader and its SRI hashes.** Written here with the hashes of the files
  that actually went into the bundle.

The wasm-bindgen output is expected beside the input wasm, as
`cargo build -p ocs_web_worker --target wasm32-unknown-unknown` leaves it; the
worker is only used when present, so the same script serves a desktop-only
bundle.
"""
import base64
import glob
import hashlib
import json
import re
import shutil
import subprocess
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CJK_FACES = ("japanese.ttf", "chinese.ttf", "traditional_chinese.ttf")
THUMBNAIL_URL = "https://i.ytimg.com/vi/{id}/mqdefault.jpg"


def subresource_integrity(path: Path) -> str:
    """The `sha384-...` value a preload needs to reject a truncated transfer."""
    digest = hashlib.sha384(path.read_bytes()).digest()
    return "sha384-" + base64.b64encode(digest).decode()


def copy_template_files(stage: Path) -> None:
    """The files the template's `data-trunk` directives name.

    A directory keeps its own name at the bundle root; everything else is
    copied flat. Getting this wrong once left `fonts/` empty on the server and
    every TTF 404'd, so the rule is explicit here.
    """
    for pattern in ("web/fonts", "web/*.js", "web/*.json", "web/*.webmanifest"):
        for source in sorted(glob.glob(str(ROOT / pattern))):
            path = Path(source)
            target = stage / path.name
            if path.is_dir():
                if target.exists():
                    shutil.rmtree(target)
                shutil.copytree(path, target)
            else:
                shutil.copy2(path, target)
    shutil.copy2(ROOT / "assets" / "logo.svg", stage / "logo.svg")


def fetch_thumbnails(stage: Path) -> None:
    """Download the Start page's video thumbnails, as the Pages workflow does.

    Best effort: without a network the cards have no picture, which is how the
    tab looked before the workflow gained this step.
    """
    catalogue = stage / "videos.json"
    if not catalogue.exists():
        return
    try:
        entries = json.loads(catalogue.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return
    thumbs = stage / "video_thumbs"
    thumbs.mkdir(exist_ok=True)
    for entry in entries:
        video_id = entry.get("id", "")
        if len(video_id) != 11:
            continue
        destination = thumbs / f"{video_id}.jpg"
        if destination.exists():
            continue
        try:
            request = urllib.request.Request(
                THUMBNAIL_URL.format(id=video_id), headers={"User-Agent": "Mozilla/5.0"}
            )
            with urllib.request.urlopen(request, timeout=15) as response:
                destination.write_bytes(response.read())
        except Exception:
            pass


def drop_cjk_fonts(stage: Path) -> None:
    """Remove the three 10 MB CJK faces, for a phone-sized bundle.

    A drawing only asks for one of these when its text style names it, and the
    app loads fonts lazily, so a viewer that will never open such a drawing
    saves 31 MB of download.
    """
    removed = 0
    for face in CJK_FACES:
        victim = stage / "fonts" / face
        if victim.exists():
            removed += victim.stat().st_size
            victim.unlink()
    if removed:
        print(f"dropped CJK fonts: {removed / 1e6:.1f} MB")


def build_worker(stage: Path, wasm_src: Path) -> bool:
    """Bind the parse worker's wasm into `worker_pkg/`, when it has been built."""
    worker_wasm = wasm_src.parent / "ocs_web_worker.wasm"
    if not worker_wasm.exists():
        print("no parse worker wasm found; the bundle will not open drawings")
        return False
    worker_pkg = stage / "worker_pkg"
    worker_pkg.mkdir(exist_ok=True)
    subprocess.run(
        ["wasm-bindgen", "--target", "web", "--out-dir", str(worker_pkg),
         "--out-name", "ocs_web_worker", str(worker_wasm)],
        check=True,
    )
    print(f"worker {(worker_pkg / 'ocs_web_worker_bg.wasm').stat().st_size} bytes")
    return True


def stamp_service_worker(stage: Path) -> str | None:
    """Give `sw.js` a cache name of its own, so a deploy replaces the old one.

    The name is a digest of the bundle's paths and sizes: two deploys that
    differ get different names, and the worker's activate step then drops the
    cache belonging to the name before.
    """
    worker = stage / "sw.js"
    if not worker.exists():
        return None
    digest = hashlib.sha256()
    for path in sorted(stage.rglob("*")):
        if path.is_file():
            digest.update(path.relative_to(stage).as_posix().encode())
            digest.update(str(path.stat().st_size).encode())
    version = digest.hexdigest()[:12]
    text = worker.read_text(encoding="utf-8")
    text = re.sub(r'const CACHE_VERSION = "[^"]*";', f'const CACHE_VERSION = "{version}";', text)
    worker.write_text(text, encoding="utf-8")
    print(f"service worker cache version: {version}")
    return version


def write_index(stage: Path, wasm: Path, javascript: Path) -> None:
    """Turn the template into the served page.

    The `data-trunk rel="rust"` directive becomes what trunk would have
    generated: a module preload and an init call, both carrying the hashes of
    the files just written.
    """
    html = (ROOT / "web-app.html").read_text(encoding="utf-8")
    boot = (
        f'<link rel="modulepreload" href="/{javascript.name}" '
        f'crossorigin="anonymous" integrity="{subresource_integrity(javascript)}">\n'
        f'    <link rel="preload" href="/{wasm.name}" '
        f'crossorigin="anonymous" integrity="{subresource_integrity(wasm)}" '
        f'as="fetch" type="application/wasm">\n'
        '    <script type="module">\n'
        f"      import init, * as bindings from '/{javascript.name}';\n"
        f"      const wasm = await init({{ module_or_path: '/{wasm.name}' }});\n"
        "      window.wasmBindings = bindings;\n"
        '      dispatchEvent(new CustomEvent("TrunkApplicationStarted", '
        "{ detail: { wasm } }));\n"
        "    </script>"
    )
    html = re.sub(r'<link data-trunk rel="rust"[^>]*>', boot, html)
    # The icon directive is replaced rather than dropped: without a <link
    # rel="icon"> the browser asks for /favicon.ico, which no bundle contains.
    html = re.sub(
        r'<link data-trunk rel="icon"[^>]*>',
        '<link rel="icon" href="logo.svg" type="image/svg+xml" />',
        html,
    )
    html = re.sub(r"\s*<link data-trunk[^>]*>", "", html)
    (stage / "index.html").write_text(html, encoding="utf-8")


def main(argv: list[str]) -> int:
    positional = [argument for argument in argv if not argument.startswith("--")]
    if len(positional) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    wasm_src, out = Path(positional[0]), Path(positional[1])
    if not wasm_src.is_file():
        print(f"no such wasm: {wasm_src}", file=sys.stderr)
        return 1

    if out.exists():
        shutil.rmtree(out)
    stage = out / ".stage"
    stage.mkdir(parents=True)

    subprocess.run(
        ["wasm-bindgen", "--target", "web", "--out-dir", str(stage),
         "--out-name", "OpenCADStudio", str(wasm_src)],
        check=True,
    )
    wasm = stage / "OpenCADStudio_bg.wasm"
    javascript = stage / "OpenCADStudio.js"
    print(f"wasm {wasm.stat().st_size} bytes")

    copy_template_files(stage)
    fetch_thumbnails(stage)
    if "--drop-cjk-fonts" in argv:
        drop_cjk_fonts(stage)
    build_worker(stage, wasm_src)
    stamp_service_worker(stage)
    write_index(stage, wasm, javascript)

    # Trunk's dist layout puts the files at the root; the stage is only a
    # placeholder until every piece is in place.
    for entry in stage.iterdir():
        shutil.move(str(entry), str(out / entry.name))
    stage.rmdir()

    total = sum(path.stat().st_size for path in out.rglob("*") if path.is_file())
    print(f"bundle {total / 1e6:.1f} MB in {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
