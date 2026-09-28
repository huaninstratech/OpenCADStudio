#!/usr/bin/env python
"""Clone một sheet (block ref + mọi đối tượng lỏng đè lên nó) thành N bản mới.

Cách dùng (server REST đang chạy, ví dụ `OpenCADStudio.exe --http 7939`):
    python scripts/clone_sheets.py <file.dwg> <số bản> <khoảng cách X>

Mỗi bản mới là 1 block định nghĩa riêng (SHEET_2, SHEET_3, …) chứa bản sao của
block ref gốc + TOÀN BỘ entity lỏng nằm đè lên sheet (image, OLE, circle,
polyline, text nhãn…) — vì những entity đó KHÔNG thuộc block, chỉ trùng vị trí.

Quy trình mỗi bản:
  1. entities_transform action=copy vector=[dx,0,0]      — clone ref + lỏng
  2. block_define name=SHEET_k base=[dx,0,0] insert_at=… — gói thành block mới
"""
import json
import sys
import urllib.request

BASE = "http://127.0.0.1:7939/api/v1"
MODEL_SPACE_OWNER = 2  # handle block record *Model_Space


def call(op, body, method="POST"):
    data = json.dumps(body).encode()
    req = urllib.request.Request(
        f"{BASE}/{op}", data=data, headers={"Content-Type": "application/json"},
        method=method,
    )
    with urllib.request.urlopen(req, timeout=600) as r:
        return json.load(r)


def get(path):
    with urllib.request.urlopen(f"{BASE}/{path}", timeout=300) as r:
        return json.load(r)


def main():
    path, count, spacing = sys.argv[1], int(sys.argv[2]), float(sys.argv[3])
    call("documents", {"path": path.replace("\\", "/")})
    doc = get("state")["document_id"]

    # mọi entity ở model space (ref + đối tượng lỏng đè lên sheet).
    # Query trả tối đa 1000 entity mỗi lần — PHẢI paginate bằng offset,
    # nếu không sẽ sót entity có handle lớn (vd các line hệ trục).
    loose = []
    off = 0
    while True:
        page = get(f"entities?detail=full&limit=1000&offset={off}")
        ents = page.get("entities", [])
        for e in ents:
            owner = (e.get("properties", {}).get("common", {}) or {}).get("owner_handle")
            if owner == MODEL_SPACE_OWNER:
                loose.append(e["handle"])
        nxt = page.get("next_offset")
        if nxt is None or not ents:
            break
        off = nxt
    print(f"entity model space: {len(loose)}")

    for i in range(1, count + 1):
        dx = i * spacing
        cp = call("entities_transform", {
            "protocol": 1, "request_id": f"clone-{i}", "document_id": doc,
            "handles": loose, "action": "copy", "vector": [dx, 0, 0],
        })
        assert cp["ok"], cp
        df = call("block_define", {
            "protocol": 1, "request_id": f"define-{i}", "document_id": doc,
            "name": f"SHEET_{i + 1}", "base": [dx, 0, 0], "insert_at": [dx, 0, 0],
            "handles": cp["result"]["created"],
        })
        assert df["ok"], df
        print(f"SHEET_{i + 1} @ x={dx}: OK")
    print("xong — dùng POST /api/v1/save để lưu file.")


if __name__ == "__main__":
    main()
