#!/usr/bin/env bash
# End-to-end proof against the live REST server:
# 1. block_define MARK from a line
# 2. clone the MARK ref twice via entities_create (INSERT)
# 3. block_define each clone into its own NEW block (B0, B1)
# 4. save -> open (reload)
# 5. verify MARK/B0/B1 definitions survived via records?collection=block_records
#    and the refs via GET /entities?type=Insert
set -euo pipefail
BASE=http://127.0.0.1:7939/api/v1
TMP="$(cygpath -m "$LOCALAPPDATA")/Temp/ocs_probe_$$"; mkdir -p "$TMP"
DOC=""
step() { printf '\n== %s\n' "$1"; }

jqget() { python -c "import sys,json; d=json.load(sys.stdin); print(json.dumps(d, indent=1)[:600])"; }

step "new drawing"
curl -s -X POST "$BASE/new" -H 'Content-Type: application/json' -d '{}' | jqget

step "draw a line"
curl -s -X POST "$BASE/commands" -H 'Content-Type: application/json' -d '{"cmd":"LINE 0,0 10,0"}' | jqget

step "query the line handle"
H=$(curl -s "$BASE/entities?type=Line" | python -c "import sys,json; d=json.load(sys.stdin); print(d['entities'][0]['handle'])")
echo "line handle: $H"

step "block_define MARK"
curl -s -X POST "$BASE/blocks" -H 'Content-Type: application/json' \
  -d "{\"name\":\"MARK\",\"base\":[0,0,0],\"handles\":[\"$H\"]}" | jqget

DOC=$(curl -s "$BASE/state" | python -c "import sys,json; print(json.load(sys.stdin)['document_id'])")
echo "document_id: $DOC"

step "clone the ref twice (entities_create INSERT)"
CLONES=$(curl -s -X POST "$BASE/entities_create" -H 'Content-Type: application/json' \
  -d "{\"protocol\":1,\"request_id\":\"c1-$$\",\"document_id\":$DOC,\"entities\":[{\"type\":\"Insert\",\"block\":\"MARK\",\"position\":[40,0]},{\"type\":\"Insert\",\"block\":\"MARK\",\"position\":[80,0]}]}")
echo "$CLONES" | jqget
C0=$(echo "$CLONES" | python -c "import sys,json; print(json.load(sys.stdin)['result']['handles'][0])")
C1=$(echo "$CLONES" | python -c "import sys,json; print(json.load(sys.stdin)['result']['handles'][1])")

step "block_define each clone into a NEW block"
curl -s -X POST "$BASE/blocks" -H 'Content-Type: application/json' \
  -d "{\"protocol\":1,\"request_id\":\"b0-$$\",\"document_id\":$DOC,\"name\":\"B0\",\"base\":[0,0,0],\"handles\":[\"$C0\"]}" | jqget
curl -s -X POST "$BASE/blocks" -H 'Content-Type: application/json' \
  -d "{\"protocol\":1,\"request_id\":\"b1-$$\",\"document_id\":$DOC,\"name\":\"B1\",\"base\":[0,0,0],\"handles\":[\"$C1\"]}" | jqget

step "save"
curl -s -X POST "$BASE/save" -H 'Content-Type: application/json' -d "{\"path\":\"$TMP/clone_test.dwg\"}" | jqget

step "reopen (reload)"
curl -s -X POST "$BASE/documents" -H 'Content-Type: application/json' -d "{\"path\":\"$TMP/clone_test.dwg\"}" | jqget

step "definitions after reload (block_records)"
curl -s "$BASE/records?collection=block_records" | python -c "
import sys, json
d = json.load(sys.stdin)
names = sorted({r['name'] for r in d.get('records', []) if r.get('name') and not r['name'].startswith('*')})
print('user definitions:', names)
assert 'MARK' in names and 'B0' in names and 'B1' in names, 'DEFINITION LOST'
print('OK: MARK/B0/B1 all present after save+reload')"

step "refs after reload"
curl -s "$BASE/entities?type=Insert" | python -c "
import sys, json
d = json.load(sys.stdin)
from collections import Counter
c = Counter(e['block'] for e in d['entities'])
print('refs by block:', dict(c))"

echo
echo "artifacts: $TMP/clone_test.dwg"
