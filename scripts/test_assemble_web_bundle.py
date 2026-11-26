"""Tests for scripts/assemble-web-bundle.py.

The assembly used to be a scratch script, and three of its mistakes reached a
deployed server before anyone noticed:

* the parse worker's wasm was never bound, so every drawing open stalled at
  "parsing entities... 10%";
* the font directory skipped itself, so every TTF 404'd;
* the service worker kept `CACHE_VERSION = "v1"`, so a returning browser was
  served the previous deploy's page.

Each test below holds one of those down. They run against a made-up checkout:
the real one would need a 203 MB wasm and a network.
"""

import importlib.util
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent


def load():
    spec = importlib.util.spec_from_file_location("assemble_web_bundle", HERE / "assemble-web-bundle.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


assemble = load()


class FakeCheckout:
    """A checkout whose REPO points at a temporary tree."""

    def __init__(self, root: Path):
        self.root = root
        (root / "web" / "fonts").mkdir(parents=True)
        (root / "web" / "fonts" / "latin.ttf").write_bytes(b"font")
        (root / "web" / "fonts" / "japanese.ttf").write_bytes(b"cjk" * 100)
        (root / "web" / "fonts" / "chinese.ttf").write_bytes(b"cjk" * 100)
        (root / "web" / "fonts" / "traditional_chinese.ttf").write_bytes(b"cjk" * 100)
        (root / "web" / "ocs-control.js").write_text("// control", encoding="utf-8")
        (root / "web" / "videos.json").write_text(
            json.dumps([{"id": "QFDMSRNrZOU", "title": "one"}, {"id": "short"}]), encoding="utf-8"
        )
        (root / "web" / "sw.js").write_text(
            'const CACHE_VERSION = "v1"; // stamped\nconst CACHE_NAME = `ocs-app-${CACHE_VERSION}`;',
            encoding="utf-8",
        )
        (root / "assets").mkdir()
        (root / "assets" / "logo.svg").write_bytes(b"<svg/>")
        (root / "web-app.html").write_text(
            '<html><head><link data-trunk rel="rust" data-bin="OpenCADStudio" />'
            '<link data-trunk rel="copy-dir" href="web/fonts" /></head><body></body></html>',
            encoding="utf-8",
        )

    def __enter__(self):
        self.directory = mock.patch.object(assemble, "ROOT", self.root)
        self.directory.start()
        return self

    def __exit__(self, *exc):
        self.directory.stop()
        return False


class TheFontDirectorySurvives(unittest.TestCase):
    """A regression left the deployed bundle with an empty fonts/."""

    def test_a_directory_keeps_its_own_name_and_contents(self):
        with tempfile.TemporaryDirectory() as temp:
            with FakeCheckout(Path(temp) / "checkout") as checkout:
                stage = Path(temp) / "stage"
                stage.mkdir()
                assemble.copy_template_files(stage)
                self.assertTrue((stage / "fonts" / "latin.ttf").is_file())
                # Files are flat, directories are not.
                self.assertTrue((stage / "ocs-control.js").is_file())
                self.assertTrue((stage / "logo.svg").is_file())


class TheCjkFacesAreOptional(unittest.TestCase):
    def test_dropping_them_keeps_the_latin_face(self):
        with tempfile.TemporaryDirectory() as temp:
            with FakeCheckout(Path(temp) / "checkout"):
                stage = Path(temp) / "stage"
                stage.mkdir()
                assemble.copy_template_files(stage)
                assemble.drop_cjk_fonts(stage)
                names = {path.name for path in (stage / "fonts").iterdir()}
                self.assertEqual(names, {"latin.ttf"})


class TheServiceWorkerGetsItsOwnCacheName(unittest.TestCase):
    """An unstamped worker served the previous deploy's page forever."""

    def test_the_stamp_replaces_the_placeholder(self):
        with tempfile.TemporaryDirectory() as temp:
            stage = Path(temp) / "stage"
            stage.mkdir()
            (stage / "sw.js").write_text(
                'const CACHE_VERSION = "v1"; // stamped\n', encoding="utf-8"
            )
            version = assemble.stamp_service_worker(stage)
            written = (stage / "sw.js").read_text(encoding="utf-8")
            self.assertEqual(len(version), 12)
            self.assertIn(f'const CACHE_VERSION = "{version}"', written)
            self.assertNotIn('"v1"', written)

    def test_two_different_bundles_get_different_names(self):
        with tempfile.TemporaryDirectory() as temp:
            first = Path(temp) / "a"
            second = Path(temp) / "b"
            for folder, size in ((first, 1), (second, 2)):
                folder.mkdir()
                (folder / "sw.js").write_text('const CACHE_VERSION = "v1";', encoding="utf-8")
                (folder / "asset.bin").write_bytes(b"x" * size)
            self.assertNotEqual(
                assemble.stamp_service_worker(first), assemble.stamp_service_worker(second)
            )


class TheThumbnailsAreFetchedWhenPossible(unittest.TestCase):
    """They are not in the repository; the Pages workflow downloads them."""

    def test_a_failed_download_leaves_the_bundle_usable(self):
        with tempfile.TemporaryDirectory() as temp:
            stage = Path(temp) / "stage"
            stage.mkdir()
            (stage / "videos.json").write_text(
                json.dumps([{"id": "QFDMSRNrZOU"}]), encoding="utf-8"
            )
            with mock.patch.object(assemble.urllib.request, "urlopen", side_effect=OSError):
                assemble.fetch_thumbnails(stage)  # must not raise
            self.assertTrue((stage / "video_thumbs").is_dir())
            self.assertEqual(list((stage / "video_thumbs").iterdir()), [])

    def test_a_short_id_is_skipped(self):
        with tempfile.TemporaryDirectory() as temp:
            stage = Path(temp) / "stage"
            stage.mkdir()
            (stage / "videos.json").write_text(json.dumps([{"id": "short"}]), encoding="utf-8")
            with mock.patch.object(assemble.urllib.request, "urlopen") as opener:
                assemble.fetch_thumbnails(stage)
            opener.assert_not_called()


class TheIndexCarriesTheHashesOfWhatWasBuilt(unittest.TestCase):
    """SRI is what makes a truncated transfer fail loudly, not silently."""

    def test_the_loader_names_the_assets_it_was_given(self):
        with tempfile.TemporaryDirectory() as temp:
            with FakeCheckout(Path(temp) / "checkout"):
                stage = Path(temp) / "stage"
                stage.mkdir()
                wasm = stage / "OpenCADStudio_bg.wasm"
                javascript = stage / "OpenCADStudio.js"
                wasm.write_bytes(b"\0asm")
                javascript.write_text("export default 1;", encoding="utf-8")
                assemble.write_index(stage, wasm, javascript)
                html = (stage / "index.html").read_text(encoding="utf-8")
                self.assertIn("/OpenCADStudio.js", html)
                self.assertIn("/OpenCADStudio_bg.wasm", html)
                self.assertIn("integrity=\"sha384-", html)
                self.assertIn("module_or_path", html)
                # The template's own directives are gone: they would otherwise
                # be served to the browser as meaningless attributes.
                self.assertNotIn("data-trunk", html)


if __name__ == "__main__":
    unittest.main()
