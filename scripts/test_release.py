"""Run with python3 scripts/test_release.py; uses only local temporary repositories."""

from contextlib import redirect_stdout
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest
from unittest.mock import patch

import release


class ReleaseTests(unittest.TestCase):
    def test_web_source_selection(self):
        workflow = (Path(__file__).resolve().parents[1] / ".github/workflows/pages.yml").read_text()
        source = workflow.split("      - name: Resolve release source\n", 1)[1]
        script = textwrap.dedent(source.split("        run: |\n", 1)[1].split("\n      - uses:", 1)[0])
        mock_gh = 'gh() { if [ "$1" = release ]; then echo v2026.37; else echo release-sha; fi; }\n'
        cases = (
            ({}, "release-sha", None),
            ({"RELEASE_TAG": "v2026.37", "RELEASE_COMMIT": "release-sha"}, "release-sha", None),
            ({"RELEASE_COMMIT": "wrong-sha"}, None, "Release tag and commit do not match"),
            ({"BUILD_MAIN": "true"}, "main-sha", None),
            ({"BUILD_MAIN": "true", "GITHUB_REF": "refs/heads/other"}, None, "Web hotfixes must run on main"),
        )
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "output"
            for overrides, commit, error in cases:
                with self.subTest(overrides=overrides):
                    output.write_text("")
                    result = subprocess.run(["bash", "-e", "-c", mock_gh + script], text=True, capture_output=True, env={
                        **os.environ, "GITHUB_REPOSITORY": "owner/repo", "GITHUB_REF": "refs/heads/main",
                        "GITHUB_SHA": "main-sha", "GITHUB_OUTPUT": str(output),
                        "RELEASE_TAG": "", "RELEASE_COMMIT": "", "BUILD_MAIN": "false", **overrides,
                    })
                    if error:
                        self.assertNotEqual(result.returncode, 0)
                        self.assertIn(error, result.stdout)
                        self.assertEqual(output.read_text(), "")
                    else:
                        self.assertEqual(result.returncode, 0, result.stderr)
                        self.assertEqual(output.read_text(), f"tag=v2026.37\ncommit={commit}\n")

    def test_build_metadata(self):
        with tempfile.TemporaryDirectory() as temp:
            binary = str(Path(temp) / "build-script")
            subprocess.run(["rustc", str(Path(__file__).resolve().parents[1] / "build.rs"), "-o", binary], check=True)
            for cargo in ("0.9.8", "2026.9.0", "2026.35.0", "2026.53.0", "2027.1.0"):
                result = subprocess.check_output([binary], text=True, env={**os.environ, "CARGO_PKG_VERSION": cargo})
                self.assertIn(f"cargo:rustc-env=OCS_APP_VERSION={release.display_version(cargo)}\n", result)

    def test_calendar_versions(self):
        self.assertEqual(release.versions("v2026.35"), {
            "version": "2026.35", "cargo": "2026.35.0", "msi": "26.35.0", "tag": "v2026.35",
        })
        self.assertEqual(release.versions("2026.09")["cargo"], "2026.9.0")
        self.assertEqual(release.versions("v2026.49.1"), {
            "version": "2026.49.1", "cargo": "2026.49.1", "msi": "26.49.1", "tag": "v2026.49.1",
        })
        self.assertEqual(release.versions("2026.54")["cargo"], "2026.54.0")
        self.assertEqual(release.display_version("2026.9.0"), "2026.09")
        self.assertEqual(release.display_version("2026.49.1"), "2026.49.1")
        self.assertEqual(release.display_version("0.9.8"), "0.9.8")
        for value in ("2026.00", "2026.9", "2026.49.0", "2026.49.65536", "bad", "0.09.8"):
            with self.assertRaises(ValueError, msg=value):
                release.versions(value)

    def test_release_commit_push_and_retry(self):
        original_run = release.run
        releases = {
            "v2026.34": {
                "name": "2026.34", "body": "Previous notes", "isDraft": False,
                "assets": [{"name": "OpenCADStudio-v2026.34-windows-x86_64-installer.msi"}],
            },
        }
        fail_create = False

        def run(*args):
            nonlocal fail_create
            if args[0] != "gh":
                return original_run(*args)
            if args[1:3] == ("release", "list"):
                return json.dumps([{"tagName": tag, "isDraft": False} for tag in releases])
            if args[1:3] == ("release", "create"):
                if fail_create:
                    fail_create = False
                    raise RuntimeError("Temporary release API failure")
                tag = args[3]
                releases[tag] = {
                    "name": args[args.index("--title") + 1],
                    "body": Path(args[args.index("--notes-file") + 1]).read_text(encoding="utf-8"),
                    "isDraft": False,
                    "assets": [{"name": "placeholder"}],
                }
                return ""
            if args[1:3] == ("release", "view"):
                tag, fields = args[3], args[5].split(",")
                return json.dumps({field: releases[tag][field] for field in fields})
            raise AssertionError(f"unexpected gh call: {args}")

        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            root = temp / "checkout"
            root.mkdir()
            previous_directory = Path.cwd()
            os.chdir(root)
            try:
                original_run("git", "init", "-b", "main")
                original_run("git", "config", "user.name", "Release test")
                original_run("git", "config", "user.email", "test@example.invalid")
                original_run("git", "init", "--bare", "-b", "main", str(temp / "origin.git"))
                original_run("git", "remote", "add", "origin", str(temp / "origin.git"))
                Path("Cargo.toml").write_text('[package]\nname = "OpenCADStudio"\nversion = "2026.34.0"\n')
                Path("Cargo.lock").write_text('version = 4\n[[package]]\nname = "OpenCADStudio"\nversion = "2026.34.0"\n')
                original_run("git", "add", ".")
                original_run("git", "commit", "-m", "Initial release")
                original_run("git", "tag", "v2026.34")
                original_run("git", "push", "origin", "main", "--tags")

                with patch.object(release, "run", run), patch.dict(os.environ, {
                    "GITHUB_REPOSITORY": "owner/repo", "GITHUB_REF": "refs/heads/main", "GITHUB_OUTPUT": str(temp / "output"),
                }):
                    def prepare(publish):
                        result = io.StringIO()
                        with redirect_stdout(result):
                            release.prepare(publish)
                        return result.getvalue()

                    self.assertIn("ready=false", prepare(True))
                    Path("feature").write_text("web release source")
                    original_run("git", "add", "feature")
                    original_run("git", "commit", "-m", "Add web release synchronization")
                    preview = prepare(False)
                    self.assertIn("Add web release synchronization", preview)
                    self.assertEqual(release.cargo_version(), "2026.34.0")
                    self.assertEqual(original_run("git", "tag", "--list", "v2026.34.1"), "")

                    # The weekly build rides the newest line as v2026.34.1 and
                    # never consumes the next hand-published number (v2026.35).
                    self.assertIn("ready=true", prepare(True))
                    sha = original_run("git", "rev-parse", "HEAD")
                    self.assertEqual(release.cargo_version(), "2026.34.1")
                    self.assertIn('version = "2026.34.1"', Path("Cargo.lock").read_text())
                    self.assertEqual(original_run("git", "log", "-1", "--format=%s"), "Release v2026.34.1")
                    self.assertEqual(original_run("git", "rev-parse", "origin/main"), sha)
                    self.assertEqual(original_run("git", "rev-parse", "v2026.34.1^{commit}"), sha)
                    self.assertEqual(original_run("git", "status", "--porcelain"), "")
                    # Nothing new since the release: a rerun stands down
                    # instead of rebuilding the same tag.
                    self.assertIn("ready=false", prepare(True))

                    # A newest release whose builds never landed is retried on
                    # the same tag instead of opening another number.
                    releases["v2026.34.1"]["assets"] = []
                    retried = prepare(True)
                    self.assertIn("tag=v2026.34.1", retried)
                    self.assertIn(f"commit={sha}", retried)
                    releases["v2026.34.1"]["assets"] = [{"name": "placeholder"}]

                    Path("feature").write_text("next change")
                    original_run("git", "add", "feature")
                    original_run("git", "commit", "-m", "Fix release retry")
                    fail_create = True
                    with self.assertRaises(RuntimeError):
                        prepare(True)
                    # The tag and version commit survive; a rerun finishes the
                    # interrupted release on the same tag.
                    sha = original_run("git", "rev-parse", "HEAD")
                    self.assertIn(f"commit={sha}", prepare(True))
                    self.assertEqual(original_run("git", "rev-parse", "HEAD"), sha)
                    self.assertEqual(releases["v2026.34.2"]["name"], "2026.34.2")
            finally:
                os.chdir(previous_directory)


if __name__ == "__main__":
    unittest.main()
