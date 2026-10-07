#!/usr/bin/env python3
"""Prepare weekly releases and share package versions across build targets."""

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile


def run(*args):
    return subprocess.check_output(args, text=True, encoding="utf-8").rstrip("\n")


def gh(*args):
    return json.loads(run("gh", *args))


def versions(tag):
    version = tag.removeprefix("v")
    weekly = re.fullmatch(r"(20\d{2})\.(\d{2})", version)
    # A tag like v2026.49.1 is a follow-up release on the 2026.49 line, used
    # when the next hand-published number is already taken.
    hotfix = re.fullmatch(r"(20\d{2})\.(\d{2})\.([1-9]\d{0,4})", version)
    if weekly:
        year, line = map(int, weekly.groups())
        if not 1 <= line <= 99:
            raise ValueError(f"Invalid release version: {tag}")
        cargo = f"{year}.{line}.0"
        msi = f"{year - 2000}.{line}.0"
    elif hotfix:
        year, line, patch = map(int, hotfix.groups())
        if patch > 65535:
            raise ValueError(f"Unsupported MSI version: {version}")
        cargo = version
        msi = f"{year - 2000}.{line}.{patch}"
    elif re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version):
        cargo = msi = version
        major, minor, patch = map(int, version.split("."))
        if major > 255 or minor > 255 or patch > 65535:
            raise ValueError(f"Unsupported MSI version: {version}")
    else:
        raise ValueError(f"Invalid release version: {tag}")
    return {"version": version, "cargo": cargo, "msi": msi, "tag": f"v{version}"}


def cargo_version():
    return re.search(r'^version = "([^"]+)"', Path("Cargo.toml").read_text(encoding="utf-8"), re.M)[1]


def display_version(cargo):
    year, week, patch = cargo.split(".")
    if len(year) == 4 and year.startswith("20") and patch == "0":
        return versions(f"{year}.{int(week):02}")["version"]
    return cargo


def output(values):
    lines = "".join(f"{key}={value}\n" for key, value in values.items())
    print(lines, end="")
    if path := os.environ.get("GITHUB_OUTPUT"):
        with open(path, "a", encoding="utf-8") as stream:
            stream.write(lines)


def release_notes(previous, tag, repo, head="HEAD"):
    sections = {
        "Modeling, drawing and annotation": [],
        "Rendering, plotting and file fidelity": [],
        "Interface, web and plugins": [],
        "Maintenance": [],
    }
    entries = run("git", "log", "--first-parent", "--format=%s%x1f%b%x1e", f"{previous}..{head}")
    for entry in entries.split("\x1e"):
        if "\x1f" not in entry:
            continue
        subject, body = entry.strip("\n").split("\x1f", 1)
        if subject.startswith("Merge pull request"):
            subject = next((line for line in body.splitlines() if line.strip()), subject)
        if subject.startswith(("Merge ", "Release v")):
            continue
        subject = re.sub(r"^(?:feat|fix|perf|chore|docs)(?:\([^)]*\))?:\s*", "", subject)
        text = subject.lower()
        if re.search(r"render|plot|print|gpu|buffer|save|file|serializ|vram", text):
            section = "Rendering, plotting and file fidelity"
        elif re.search(r"ui|web|plugin|ribbon|theme|translat|dropdown|mcp|control|cli", text):
            section = "Interface, web and plugins"
        elif re.search(r"kernel|curve|solid|surface|extrud|loft|sweep|revol|presspull|snap|hatch|grip|trim|offset|draw|dimension", text):
            section = "Modeling, drawing and annotation"
        else:
            section = "Maintenance"
        if subject not in sections[section]:
            sections[section].append(subject)
    lines = ["## Highlights", "", f"- Weekly {tag.removeprefix('v')} release for web and desktop from the same source revision.", ""]
    for title, subjects in sections.items():
        if subjects:
            lines += [f"## {title}", ""]
            # ponytail: six recent entries per section; full history is linked below.
            lines += [f"- {subject}" for subject in subjects[:6]]
            lines += [""]
    lines += [f"**Full Changelog:** https://github.com/{repo}/compare/{previous}...{tag}", ""]
    return "\n".join(lines)


def prepare(publish):
    if run("git", "status", "--porcelain"):
        raise ValueError("Release preparation requires a clean working tree")
    repo = os.environ["GITHUB_REPOSITORY"]
    if publish and os.environ.get("GITHUB_REF") != "refs/heads/main":
        raise ValueError("Weekly releases must run on main")
    releases = [release for release in gh("release", "list", "--limit", "100", "--json", "tagName,isDraft") if not release["isDraft"]]
    published_tags = {release["tagName"] for release in releases}
    numbered = []
    for release in releases:
        match = re.fullmatch(r"v(20\d{2})\.(\d{2})(?:\.([1-9]\d{0,4}))?", release["tagName"])
        if match:
            year, line, hotfix = match.groups()
            numbered.append((int(year), int(line), int(hotfix or 0)))
    if not numbered:
        raise ValueError("No numbered release found to continue the sequence from")
    year, line, hotfix = max(numbered)
    newest = f"v{year}.{line}" + (f".{hotfix}" if hotfix else "")

    # The weekly build hangs off the newest release line as v2026.N.1, .2, ...
    # so it never consumes the next hand-published number. A newest release
    # without build assets is retried on its own tag; a pushed tag without a
    # release is an interrupted run to finish on the same tag.
    tag = None
    reused = False
    if not gh("release", "view", newest, "--json", "assets")["assets"]:
        tag, reused = newest, True
    elif run("git", "rev-list", "--count", f"{newest}..HEAD") == "0":
        output({"ready": "false"})
        return
    else:
        for attempt in range(1, 100):
            candidate = f"v{year}.{line}.{attempt}"
            if candidate in published_tags:
                continue
            tag = candidate
            reused = bool(run("git", "tag", "--list", candidate))
            break
        if tag is None:
            raise ValueError(f"No free hotfix number after {newest}")

    version = versions(tag)
    if reused and tag in published_tags:
        # A retry always uses the original tag, even when main has advanced.
        sha = run("git", "rev-parse", f"{tag}^{{commit}}")
        published = gh("release", "view", tag, "--json", "name,isDraft,body")
        if published["isDraft"] or not published["body"].strip() or published["name"] != version["version"]:
            raise ValueError("Existing weekly release metadata is invalid")
        output({"ready": str(publish).lower(), "tag": tag, "commit": sha})
        return
    notes = release_notes(newest, tag, repo, tag if reused else "HEAD")
    print(notes)
    if not publish:
        output({"ready": "false", **version, "commit": run("git", "rev-parse", "HEAD")})
        return
    if not reused:
        old = cargo_version()
        updates = {}
        for path, prefix in ((Path("Cargo.toml"), ""), (Path("Cargo.lock"), 'name = "OpenCADStudio"\n')):
            content = path.read_text(encoding="utf-8")
            before = f'{prefix}version = "{old}"'
            if before not in content:
                raise ValueError(f"Cannot update version in {path}")
            updates[path] = content.replace(before, f'{prefix}version = "{version["cargo"]}"', 1)
        for path, content in updates.items():
            path.write_text(content, encoding="utf-8")
        run("git", "config", "user.name", "github-actions[bot]")
        run("git", "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")
        run("git", "add", "Cargo.toml", "Cargo.lock")
        run("git", "commit", "-m", f"Release {tag}")
        run("git", "tag", "-a", tag, "-m", f"Release {tag}")
        run("git", "push", "--atomic", "origin", "HEAD:main", f"refs/tags/{tag}")
        sha = run("git", "rev-parse", "HEAD")
    else:
        sha = run("git", "rev-parse", f"{tag}^{{commit}}")
    # Windows cannot read a NamedTemporaryFile that is still open, so the
    # notes file is closed before the gh call (and tests read it back).
    notes_path = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".md", encoding="utf-8", delete=False) as notes_file:
            notes_file.write(notes)
            notes_path = Path(notes_file.name)
        run("gh", "release", "create", tag, "--verify-tag", "--title", version["version"], "--notes-file", str(notes_path), "--latest")
        published = gh("release", "view", tag, "--json", "name,body,isDraft")
    finally:
        if notes_path is not None:
            notes_path.unlink(missing_ok=True)
    if published != {"name": version["version"], "body": notes, "isDraft": False}:
        raise ValueError("Published release notes do not match the prepared release")
    output({"ready": "true", "tag": tag, "commit": sha})


def verify_native(tag):
    release = gh("release", "view", tag, "--json", "assets,body,isDraft")
    expected = {
        f"OpenCADStudio-{tag}-{suffix}"
        for suffix in ("linux-x86_64.AppImage", "linux-x86_64.snap",
                       "windows-x86_64-portable.exe", "windows-x86_64-installer.msi", "macos-arm64.dmg")
    }
    uploaded = {asset["name"] for asset in release["assets"] if asset["size"] > 0 and asset["state"] == "uploaded"}
    if release["isDraft"] or not release["body"].strip() or not expected <= uploaded:
        raise ValueError(f"Incomplete native release: {sorted(expected - uploaded)}")
    print(f"Verified native release {tag}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    prepare_parser = commands.add_parser("prepare")
    prepare_parser.add_argument("--publish", action="store_true")
    version_parser = commands.add_parser("versions")
    version_parser.add_argument("--tag", default="")
    verify_parser = commands.add_parser("verify-native")
    verify_parser.add_argument("--tag", required=True)
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.publish)
    elif args.command == "verify-native":
        verify_native(args.tag)
    else:
        version = versions(args.tag or display_version(cargo_version()))
        if args.tag and version["cargo"] != cargo_version():
            raise ValueError("Release tag and Cargo version do not match")
        output(version)


if __name__ == "__main__":
    main()
