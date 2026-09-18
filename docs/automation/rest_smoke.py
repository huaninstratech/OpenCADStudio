#!/usr/bin/env python3
"""Black-box smoke test for the REST transport (`--http <port>`).

Drives the whole entity lifecycle over plain HTTP — the same API surface any
language can call — against a real drawing session: create → verify by query
→ transform → mark with xdata → define a block → save → reopen → verify
everything persisted → erase → undo. Uses only the Python standard library.

    python3 docs/automation/rest_smoke.py target/debug/OpenCADStudio
"""

import json
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path


def call(port: int, method: str, path: str, body=None):
    """One HTTP call; returns (status, parsed-json-or-None)."""
    data = None
    headers = {"Content-Type": "application/json"}
    if body is not None:
        data = json.dumps(body).encode()
    request = urllib.request.Request(
        f"http://127.0.0.1:{port}/api/v1{path}", data=data, headers=headers, method=method
    )
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            raw = response.read()
            return response.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as error:
        raw = error.read()
        return error.code, (json.loads(raw) if raw else None)


def wait_ready(process, port: int, timeout: float = 60.0) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if process.poll() is not None:
            raise SystemExit(f"server exited early: {process.returncode}")
        try:
            status, body = call(port, "GET", "/ready")
            if status == 200 and body.get("ok"):
                return
        except (urllib.error.URLError, ConnectionError, OSError):
            time.sleep(0.25)
    raise SystemExit("server never became ready")


def expect(condition, message):
    if not condition:
        raise SystemExit(f"FAIL: {message}")


def main() -> None:
    exe = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/OpenCADStudio").resolve()
    port = 8091
    out_dir = Path(tempfile.mkdtemp(prefix="ocs_rest_smoke_"))
    process = subprocess.Popen(
        [str(exe), "--http", str(port)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        wait_ready(process, port)

        # 0. Readiness + capability discovery.
        status, ready = call(port, "GET", "/ready")
        expect(status == 200 and ready["session_id"], f"ready: {ready}")
        status, capabilities = call(port, "GET", "/capabilities")
        expect(capabilities["operations"]["entities_create"] is True, "capabilities")

        # 1. Create a batch of typed entities; the missing layer is created.
        status, created = call(port, "POST", "/entities", {
            "entities": [
                {"type": "Line", "start": [0, 0], "end": [100, 0], "layer": "FRAME"},
                {"type": "LwPolyline", "vertices": [[0, 0], [100, 0], [100, 60], [0, 60]], "closed": True},
                {"type": "Text", "value": "PAGE-01", "position": [10, 50], "height": 3.0},
                {"type": "Circle", "center": [80, 30], "radius": 5},
            ]
        })
        expect(status == 201 and created["result"]["created"] == 4, f"create: {created}")
        handles = created["result"]["handles"]
        expect(created["result"]["layers_created"] == ["FRAME"], "layer auto-created")

        # 2. Verify by query — the geometry really landed.
        status, query = call(port, "GET", "/entities?type=Line&detail=full")
        expect(query["entities"][0]["end"] == [100.0, 0.0, 0.0], f"line geometry: {query}")
        status, query = call(port, "GET", f"/entities/{handles[2]}")
        expect(query["entities"][0]["value"] == "PAGE-01", "text content by handle")

        # 3. Transform: move the circle and verify, then array the frame.
        status, moved = call(port, "POST", "/entities/transform", {
            "handles": [handles[3]], "action": "move", "vector": [10, 0],
        })
        expect(moved["result"]["affected"] == 1, f"move: {moved}")
        status, query = call(port, "GET", f"/entities/{handles[3]}")
        expect(query["entities"][0]["center"][0] == 90.0, "circle moved")

        status, copied = call(port, "POST", "/entities/transform", {
            "handles": [handles[0]], "action": "copy", "vector": [0, -10],
        })
        expect(len(copied["result"]["created"]) == 1, f"copy: {copied}")

        # 4. Mark the text with extended data (RegApp registered implicitly).
        status, marked = call(
            port, "PUT", f"/entities/{handles[2]}/xdata/SPM",
            [{"code": 1000, "value": "PAGE-01"}, {"code": 1070, "value": 3}],
        )
        expect(marked["result"]["updated"] == 1, f"xdata put: {marked}")
        status, xdata = call(port, "GET", f"/entities/{handles[2]}/xdata?app=SPM")
        expect(xdata["items"][0]["xdata"]["SPM"][0] == "PAGE-01", "xdata read")

        # 5. Define a block from the two frame lines, AutoCAD BLOCK style.
        status, block = call(port, "POST", "/blocks", {
            "name": "FRAME-MARK", "base": [0, 0, 0], "handles": [handles[0], copied["result"]["created"][0]],
        })
        expect(status == 201 and block["result"]["block"] == "FRAME-MARK", f"block: {block}")

        # 6. Save → reopen: everything survives the file round trip.
        saved = out_dir / "rest_smoke.dwg"
        status, saved_response = call(port, "POST", "/save", {"path": str(saved)})
        expect(saved_response["ok"], f"save: {saved_response}")
        status, opened = call(port, "POST", "/documents", {"path": str(saved)})
        expect(opened["ok"], f"reopen: {opened}")

        status, query = call(port, "GET", "/entities?type=Insert&detail=full")
        expect(query["count"] == 1 and query["entities"][0]["block"] == "FRAME-MARK", "insert persisted")
        status, query = call(port, "GET", "/entities?type=Text")
        text_handle = query["entities"][0]["handle"]
        status, xdata = call(port, "GET", f"/entities/{text_handle}/xdata?app=SPM")
        expect(xdata["items"][0]["xdata"]["SPM"][1] == 3, "xdata persisted")

        # 7. Erase + undo — the standard lifecycle close.
        status, erased = call(port, "DELETE", f"/entities?handles={text_handle}")
        expect(erased["result"]["erased"] == 1, f"erase: {erased}")
        status, undone = call(port, "POST", "/undo")
        expect(undone["ok"], f"undo: {undone}")
        status, query = call(port, "GET", "/entities?type=Text")
        expect(query["count"] == 1, "undo restored the text")

        print("rest smoke: OK")
    finally:
        process.terminate()
        process.wait(timeout=30)


if __name__ == "__main__":
    main()
