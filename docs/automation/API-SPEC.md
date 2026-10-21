# OpenCADStudio Automation API — Specification

Specification version **1.3** · API protocol **1** · applies to fork builds
`2026.41` and later (`2026.40` for everything except §3.5 and §6.12).
Build `2026.46` fixes REST numeric query params (`limit`/`offset` were
silently dropped before, pinning every `GET /entities` / `GET /records`
response to its first 1000 rows); §6.4 "Pagination" documents the fixed
behaviour.

Spec **1.3** makes §6 a complete per-operation reference: every mutation and
read now carries its own field table and a worked request/response example.
It adds the operations that landed after spec 1.2 — `switch_document`,
`grip_drag`, `click`, `pointer`, `text_replace`, `save_verified`, `stop` —
and documents the REST default port and descriptor discovery (§3.1).

This document is the complete, self-contained reference for the OpenCADStudio
automation API. Everything a client needs — conventions, transports, the
operation catalogue, validation rules, error codes and worked examples — lives
here. The API is transport-neutral: one operation dispatcher serves every
client, so a request has identical semantics whether it arrives over a local
HTTP port, a stdio pipe, a TCP socket or an AI-tool (MCP) session.

MCP session set-up and a hands-on REST walkthrough live in
[`docs/automation/README.md`](README.md); this file links to it rather than
repeating either.

---

## 1. Overview

| Concept | Meaning |
|---|---|
| **Session** | One running editor process. Owns a session id, an event cursor and 1…N open documents. |
| **Document** | One drawing (a tab). Identified by a stable `document_id`. Exactly one document is **active** at a time. |
| **Handle** | Identity of every database object — a hexadecimal string (e.g. `"2A"`). Handles are stable across save/open and are the key for all read, edit, erase and clone operations. |
| **Operation** | One atomic, undoable step. Validated in full before anything commits; a rejected request changes nothing. |
| **Envelope** | The mutation wrapper: `"protocol":1`, a caller-generated `request_id`, and the `document_id` under edit. |
| **Pick** | A human-in-the-loop operation (`user_select`, `getpoint`, §6.12): the operation stays pending while a *person* answers at the screen. |

Design rules, in priority order:

1. **Idempotency** — a mutation replayed with the *same* `request_id` returns
   the cached result instead of executing twice. Generate one fresh
   `request_id` per logical operation (≤ 128 bytes) and reuse it only to
   retry after a timeout.
2. **Optimistic state** — read `state` first, pass `document_id` (and
   `revision`/`geometry_revision` when you hold them) back on mutations. A
   refusal whose code is in the retryable set (§5.2) means "your snapshot is
   behind": refresh state and retry once with a *new* `request_id`.
3. **Atomicity** — every operation is all-or-nothing and produces exactly one
   undo step. Multi-step transactions are the client's `batch` op or a
   sequence of ops against a held `revision`.
4. **Geometry stays in the engine** — measure, containment, intersection and
   ranking are computed by the built-in geometry kernel; clients never
   reimplement them.

---

## 2. Conventions

| Convention | Value |
|---|---|
| Angles | **degrees** on the wire, counterclockwise positive (`rotation_deg`, `angle_deg`, `start_angle_deg`, `end_angle_deg`) |
| Points | `[x, y]` (z = 0) or `[x, y, z]`, world coordinates (WCS). Interactive input additionally accepts `"space":"ucs"` / `"relative"`. |
| Distances, sizes | drawing units (millimetres only where a field is explicitly paper-space, e.g. `paper_width`) |
| Colors | integer index `1–255` (`1` red … `7` white), `256` by-layer, `0` by-block; true color accepted where noted |
| Handles | hexadecimal strings, case-insensitive, no `0x` prefix required |
| Names | layer / block / style names are case-insensitively unique; `*`-prefixed names are reserved |
| Booleans | JSON `true`/`false` |
| Templates | a `*.dwt` file is written and read as DWG bytes — same container, template extension |
| Paper sizes | catalog names, e.g. `"ISO_A4_(210.00_x_297.00_MM)"` (canonical name is echoed back in results) |
| Scales | `"fit"` or `"paper:drawing"` ratio like `"1:100"` (both parts > 0) |

---

## 3. Transports

All transports speak to the same dispatcher; pick one per client. REST and
MCP advertise the whole surface; feature detection is `capabilities` (§4).

### 3.1 HTTP REST — headless (`--http <port>`)

`OpenCADStudio --http 8090` runs a private, window-less session behind a
resource-oriented REST surface at `http://127.0.0.1:<port>/api/v1` (loopback
bind only, JSON bodies, one request per connection, no CORS headers). A
machine-readable OpenAPI 3 description of the whole surface is served at
`GET /api/v1/openapi`.

Every request carries `Authorization: Bearer <token>`: the value of
`OCS_API_TOKEN` when the launcher set it, otherwise a random token printed on
stderr at startup. A request without it is answered **401**
`{"code":"unauthorized"}`. Loopback is shared by every process and user on
the machine, so the port alone is no credential.

**Default port and discovery** — `--http` is optional: a GUI launched with no
flag also serves this surface on `127.0.0.1:7931`. The channel announces
itself by writing a descriptor to the platform config folder,
`%APPDATA%\OpenCADStudio\automation\http.json` on Windows
(`…/OpenCADStudio/automation/http.json` under the equivalent config dir on
Linux/macOS):

```json
{"protocol":1,"pid":12345,"port":7931,"token":"…","default":true}
```

| Field | Meaning |
|---|---|
| `port` | The live port — same value as `--http` when the flag was given |
| `token` | The bearer token to send on every request (§3.2 uses the same) |
| `pid` | Owning process id — a client can check the channel is alive before calling |
| `default` | `true` when this channel is the no-flag default port (7931), `false` for an explicit `--http` |

So the person can open a drawing from the desktop shortcut and a script
discovers the running API without any preset env var: read `http.json`, send
`Authorization: Bearer <token>` to `127.0.0.1:<port>/api/v1`. An explicit
`--http PORT` overrides the default; when the default port is already taken
the later instance logs a hint on stderr and serves nothing (`--http` on a
free port always works). The descriptor is rewritten every time a channel
starts listening, so the newest editor wins the file.

| Status | Meaning |
|---|---|
| `200` | Success (read or update) |
| `201` | A resource was created (document, entities, block, group, selection set, layout) |
| `400` | Validation failure — the body keeps the symbolic `code` and message |
| `401` | Missing or wrong bearer token (`code:"unauthorized"`) |
| `404` | Unknown route or unknown handle (`code:"entity_absent"`) |
| `409` | State conflict (`stale_state`, `document_not_active`) or GUI required (`gui_required`) |
| `503` | Dispatcher busy |

The REST layer generates `request_id`s and caches the active `document_id`
server-side, retrying once on retryable refusals — plain HTTP clients do not
need envelope bookkeeping. Power clients may instead POST any operation name
to `/api/v1/{op}` with an explicit body — including `operation` (poll a
pending op) and `cancel`.

Connections are served one thread each: a slow op (`plot`, opening a large
drawing) or a client that connects and stalls holds only its own connection,
never the other callers. The ops themselves still execute one at a time on
the private session (the same one-dispatcher pipeline as every transport), so
the concurrency buys responsiveness, not parallel mutation. A connection that
sends nothing for **15 s** is dropped.

### 3.2 HTTP REST — GUI-hosted (`--http <port>` **with a drawing file**)

`OpenCADStudio.exe "<file>" --http 8090` boots the **normal editor** and hosts
the same REST surface on this very process, aimed at the drawing the person at
the screen is working on: identical routes, identical semantics, plus the
human-in-the-loop picks of §6.12 (which need the window this process owns).
Differences worth knowing:

- `GET /state` primes a server-side `document_id` cache; later mutations
  address the live drawing without the caller repeating the id. A stale cache
  (the person switched documents) costs one state refresh and one retry, not
  an error.
- `session_id` is not needed (the channel has no descriptor handshake; a
  guessed value is stripped).
- One thread per connection: a parked pick never blocks other calls. A pick
  may park up to **30 minutes**; any other op answers within **300 s** or the
  bridge replies `response_timeout`.
- A parked pick watches its own connection: the moment the client hangs up
  (its own timeout, a crash, a killed probe), the pending pick is retracted
  with `cancel` and the channel is free again — the next pick starts clean
  instead of dying on `interactive_pending` until the person happens to press
  Esc. The same retraction fires if the caller waits out the full 30 minutes.
- Every pick logs its lifecycle to stderr as `[pick] <op> <request_id>` lines
  (start, cancelled-by-disconnect, settled), one per state, so a client-side
  log can be cross-referenced line for line.
- Unknown POST paths are treated as op attempts and forwarded (any op of the
  surface works — including `operation` to poll a pending pick and `cancel`
  to retract it); truly unrouted paths answer `404 {"code":"unknown_route"}`.
- The same bearer token as §3.1 is required on every request.

### 3.3 stdio JSONL (`--serve`)

One JSON value per line over stdin/stdout. The **first** stdout line is the
ready greeting: `{"ok":true,"ready":true,"version":"…","session_id":"…"}`.
Every subsequent line is one request/response pair (legacy reads need no
envelope; mutations use the envelope of §1).

### 3.4 TCP (`--serve --port N`)

The same JSONL protocol as §3.3 over `127.0.0.1:<N>`, one client at a time.
Every request line carries `"token":"<token>"` (the §3.1 token); a line
without it is answered `{"ok":false,"code":"unauthorized"}` and the
connection is closed.

### 3.5 AI-tool session (`--mcp`)

Exposes four tools — `ocs_sessions`, `ocs_read`, `ocs_execute`, `ocs_capture`
— with JSON-Schema validated arguments (`ocs_execute` alone accepts all 44
mutation operations, §6). `ocs_read` additionally serves the generated
per-type creation schema (`entity_schema`) and on-demand guides. Session
discovery and capability advertisement follow the same protocol-1 semantics.
Set-up instructions: see the MCP section of [`README.md`](README.md).

---

## 4. Session and state

`{"protocol":1,"op":"state"}` (or `GET /api/v1/state`) returns everything a
client needs to act:

| Field | Meaning |
|---|---|
| `session_id` | Identity of the running session; required by the MCP transport, accepted-and-ignored elsewhere (only a *wrong* value is refused) |
| `document_id` | The active document |
| `revision`, `geometry_revision` | Optimistic-concurrency tokens for data and geometry |
| `hand_seed` | The drawing's handle seed, hex — a monotonically advancing per-file object counter |
| `documents[]` | Every open document: `id`, `title`, `path`, `dirty`, `revision` |
| `selection[]` | Handles currently selected, in pick order |
| `command` | An interactive command's state, when one is running: `accepts[]`, `options[]`, `input_example` |
| `operation` | The `request_id` of the operation currently pending, when one is |
| `capabilities` | Feature list (also available as the dedicated `capabilities` read) |

`hello` is an alias of `state`. `file_identity` (§6.1) additionally gives
every drawing a stable GUID.

---

## 5. Error model

### 5.1 Response envelope

Every settled response carries:

```json
{"ok":true,"status":"completed","request_id":"…","result":{…},"changes":[…],"state":{…}}
```

`status` is one of `completed`, `accepted`, `running` (async work — poll with
`{"protocol":1,"op":"operation","request_id":…}`), `waiting_input` (an
interactive command wants more input), `cancelled`, `failed`. `changes[]`
lists affected handles (bounded to 1000) so clients can refresh cheaply.

### 5.2 Error codes

Failures are `{"ok":false,"code":"…","error":"…"}`. The `code` is symbolic
and stable; branch on it, not on message text.

| Code | Meaning | Retryable |
|---|---|---|
| `stale_state`, `session_changed` | The caller's snapshot is behind | yes — refresh `state`, retry once with a new `request_id` |
| `document_required`, `document_not_active`, `document_closed` | Envelope addressing problem | yes (refresh state) |
| `document_dirty` | Closing would lose unsaved changes | no — resend with `"discard":true` to confirm |
| `entity_absent` | A listed handle does not exist (the message names them) | refresh and re-read |
| `command_busy` | An interactive command is active | finish or `cancel` first |
| `gui_required` | Operation needs the editor window (`view_focus`, and the picks of §6.12 over headless transports) | headless sessions only |
| `busy` | Async work still running | poll `operation` |
| `interactive_pending` | Another person-pick (§6.12) is already waiting for the user | wait for it, or retract it with `cancel` |
| `user_input_busy` | The person is mid-command or in a dialog; their keystrokes own Enter/Esc | wait, then retry |
| `invalid_detail` | `user_select` `detail` outside `summary`/`geometry`/`full` | no |
| `selection_changed` | Precondition `selection[]` no longer matches | re-read and confirm |
| `name_required`, `entities_required`, `selection_required`, `app_required`, `path_required`, `handles_required`, `block_required`, `missing_find` | Missing mandatory field | no |
| `absolute_path_required` | `save_verified` was given a relative path | resend with an absolute path |
| `unknown_entity_type`, `invalid_radius`, `invalid_vertices`, `invalid_point`, `invalid_text`, `invalid_scale`, `invalid_paper`, `invalid_handle`, `invalid_xdata`, `invalid_sysvar_value`, `invalid_sweep`, `unknown_entity_field` | Validation failures — nothing commits | no |
| `unknown_sysvar` | Variable outside the registry (§6.7) | no |
| `disabled` | Automation mutations are stopped (`stop` op, §6.10) — re-enable in the GUI | no (manual) |
| `no_document` | Command op (`run`/`start`/`input`) with no open drawing | create or open one |
| `block_editor_open` | `entities_create` while the block editor has a block open | finish block editing first |
| `document_not_found` | `document_id` names no open document | refresh `state` |
| `layout_exists`, `layout_missing`, `layer_failed`, `block_failed`, `block_missing`, `block_protected`, `block_referenced`, `block_in_edit`, `block_invalid_name`, `block_exists`, `selection_set_missing`, `template_missing`, `template_failed`, `save_failed` | Domain refusals, self-explanatory | no |
| `reopen_failed`, `version_mismatch`, `semantic_mismatch`, `invalid_dxf_handle_graph` | `save_verified` roundtrip checks (§6.1); the file is preserved for diagnosis | fix the source data |

Unknown routes answer `code:"unknown_route"` (HTTP 404). A timed-out parked
request answers `code:"response_timeout"`.

---

## 6. Operations reference

The surface is one catalogue served identically by every transport:
**44 mutation** ops (envelope ops, §1) and the **reads** of §6.4. All 44 are
listed in the `capabilities` advertisement and in the MCP schema; the REST
passthrough accepts any of them at `POST /api/v1/{op}`.

Every entry below has the same shape so a client can skim or deep-read:

1. **Purpose** — one line.
2. **Request fields** — name, type, required/optional, meaning. Types follow
   §2 (`Point` = `[x,y]` or `[x,y,z]`, `handles` = array of hex strings, …).
3. **Result** — what `result` carries in the settled envelope of §5.1
   (`request_id`, `changes[]`, `state` elided as `…` in the examples).
4. **Example** — a ready-to-send request and a typical response.

Ops marked **route** have a dedicated REST path (full table §6.11); every
other op rides the passthrough, whose generic form is:

```sh
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST \
  "http://127.0.0.1:7931/api/v1/$OP" -H "Content-Type: application/json" \
  -d "$BODY"
```

### 6.1 Documents, session, lifecycle

#### `new` — start a fresh untitled document

| Field | Type | Required | Meaning |
|---|---|---|---|
| `template` | string | no | Path to a `*.dwt`/`*.dwg`/`*.dxf` template — the new document inherits its tables, styles and entities and keeps no source path |

Result: `total` (entities imported from the template). REST: **route**
`POST /documents` — `{}` (or an empty body) makes an empty document,
`{"template":…}` seeds one, `{"path":…}` opens instead (next op).

```json
{"op":"new","template":"C:/tpl/a3.dwt"}
→ {"ok":true,"status":"completed","result":{"total":137},"changes":[],"state":{…}}
```

#### `open` — open an existing drawing as the active document

| Field | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | **yes** | Absolute path to a `.dwg`/`.dxf` file; it opens in its own tab and becomes active |

Result: entity summary (total and per-type counts). REST: **route**
`POST /documents` with `{"path":…}`.

```json
{"op":"open","path":"C:/drawings/plan.dwg"}
→ {"ok":true,"status":"completed","result":{"total":4210,"by_type":{"Line":3900,…}}}
```

#### `activate` · `switch_document` — change the active tab

| Field | Type | Required | Meaning |
|---|---|---|---|
| `document_id` | integer | **yes** | The open tab to make active |

`switch_document` is an alias of `activate` — identical contract, kept for
clients that prefer the explicit name.

```json
{"op":"activate","document_id":2}
```

#### `close` — close the active document

| Field | Type | Required | Meaning |
|---|---|---|---|
| `discard` | boolean | no | `true` discards unsaved changes; without it a dirty document is refused with `document_dirty` |

Closing the last tab leaves a fresh empty drawing. Result: `closed`.
REST: **route** `DELETE /documents/{id}?discard=true`.

```json
{"op":"close","discard":true}
→ {"ok":true,"status":"completed","result":{"closed":1},…}
```

#### `save` — write the active document to disk

| Field | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | while untitled | Destination; `*.dwg`/`*.dxf` follow the extension, `*.dwt` writes a template (DWG bytes) |
| `target_format` | `"dwg"` \| `"dxf"` | no | Explicit output format (must agree with the extension) |
| `target_version` | version name | no | `R14`, `2000`…`2018`, or `AC1014`…`AC1032` |
| `allow_lossy` | boolean | no | Acknowledge records the target version cannot represent (they are dropped and reported) |

Result: `saved`. REST: **route** `POST /save` with optional `{"path":…}`.

```json
{"op":"save","path":"C:/out/plan-2018.dwg","target_version":"AC1032"}
→ {"ok":true,"status":"completed","result":{"saved":"C:/out/plan-2018.dwg"},…}
```

#### `save_verified` — save, reopen and prove the roundtrip

| Field | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | **yes** | *Absolute* destination `.dwg`/`.dxf` (a relative path answers `absolute_path_required`) |
| `target_format` | `"dwg"` \| `"dxf"` | no | Explicit output format matching the extension |
| `target_version` | version name | no | Explicit CAD version (same values as `save`) |
| `allow_lossy` | boolean | no | Acknowledge dropped records |
| `overwrite` | boolean | no | Replace an existing file instead of refusing it |

The delivery-safe write: saves, reopens the written file, re-derives its
semantic manifest and compares it against the source, audits the DXF handle
graph, and answers with the SHA-256 of the output. Any mismatch refuses with
`version_mismatch` / `semantic_mismatch` / `invalid_dxf_handle_graph` and
preserves the file for diagnosis. Not batchable.

```json
{"op":"save_verified","path":"C:/out/plan-final.dwg"}
→ {"ok":true,"status":"completed","verified":true,"saved":"C:/out/plan-final.dwg",
   "sha256":"9f2c…","bytes":183442,"target_format":"dwg","target_version":"AC1032",
   "dropped_on_save":[],"manifest":{…},"audit":{…}}
```

#### `file_identity` — the drawing's stable GUID

| Field | Type | Required | Meaning |
|---|---|---|---|
| `renew` | boolean | no | Mint a fresh identity instead of returning the stored one |

Result: `identity` (RFC 4122 v4 GUID) and `created` (true on first call). The
GUID is minted on first call, persisted inside the drawing, and survives
save/open and clone. REST: **route** `POST /file-identity`.

```json
{"op":"file_identity"}
→ {"ok":true,"status":"completed","result":{"identity":"5db78b90-ca0b-4a4f-ad68-8c860483a41f","created":true},…}
```

#### `undo` · `redo`

No request fields; each is exactly one undo step on the active document.
REST: **routes** `POST /undo` · `POST /redo`.

```json
{"op":"undo"}
```

### 6.2 Entity creation — `entities_create`

`{"entities":[{…},…]}` — a batch of typed definitions, validated in full
before anything commits; one undo step; missing layers are auto-created and
reported in `result.layers_created`. Every definition also accepts `layer`
and `color` (index); any other field is refused (`unknown_entity_field`).
An MCP `ocs_read` `entity_schema` call (no `type` → the type list, or
`parameters.type` → fields and examples) echoes this table programmatically.

| Type | Fields |
|---|---|
| `Line` | `start`, `end`, optional `thickness` |
| `Circle` | `center`, `radius` (≥ 0) |
| `Arc` | `center`, `radius` (> 0), `start_angle_deg`, `end_angle_deg` (must sweep a finite non-zero arc) |
| `Spline` | `control_points` (more than `degree`) or `fit_points` (≥ 2), optional `degree` (1–10, default 3), `knots` (control points + degree + 1, non-decreasing), `weights` (one positive value per control point), `closed`, `periodic`, `rational`, `planar`, `linear` |
| `LwPolyline` (alias `Polyline`) | `vertices` (≥ 2), `closed`, optional `constant_width` |
| `Point` | `location` |
| `Text` | `value`, `position`, optional `height` (default 2.5), `rotation_deg`, `style` |
| `MText` | `value`, `position`, optional `height`, `width`, `rotation_deg`, `style` |
| `Insert` | `block` (must exist), `position`, optional `scale` (number or `[x,y,z]`, nonzero), `rotation_deg` |
| `Solid` | `corners` (3 or 4 `[x,y,z]`) |
| `Hatch` | `boundary` (≥ 3 points, closed), `solid`, or `pattern` (catalog name, default `ANSI31`), `pattern_scale`, `pattern_angle_deg` |

Result: `{"handles":[…],"created":N,"layers_created":[…]}` → HTTP 201.

Example — a mixed batch; `"FRAME"` does not exist yet and is created:

```json
{"op":"entities_create","entities":[
  {"type":"Line","start":[0,0],"end":[100,0],"layer":"FRAME"},
  {"type":"Circle","center":[50,25],"radius":10,"color":1},
  {"type":"LwPolyline","vertices":[[0,0],[100,0],[100,50],[0,50]],"closed":true},
  {"type":"Text","value":"PAGE-01","position":[10,50],"height":3.0,"rotation_deg":0},
  {"type":"Arc","center":[50,25],"radius":10,"start_angle_deg":0,"end_angle_deg":90}
]}
→ {"ok":true,"status":"completed",
   "result":{"handles":["63","64","65","66","67"],"created":5,"layers_created":["FRAME"]},…}
```

One definition per type, REST style (route is `POST /entities`):

```sh
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST \
  http://127.0.0.1:7931/api/v1/entities -H "Content-Type: application/json" \
  -d '{"entities":[{"type":"Line","start":[0,0],"end":[100,0],"layer":"FRAME"}]}'
```

### 6.3 Entity modification

#### `entities_delete` — erase by handles

| Field | Type | Required | Meaning |
|---|---|---|---|
| `handles` | string[] | **yes** | Hex handles to erase; every one must exist — otherwise nothing is erased and `entity_absent` names the missing |

Result: `erased` (count). REST: **route** `DELETE /entities?handles=63,64`.

```json
{"op":"entities_delete","handles":["63","64"]}
→ {"ok":true,"status":"completed","result":{"erased":2},…}
```

#### `entities_transform` — move / copy / rotate / scale / mirror / array

| Field | Type | Required | Meaning |
|---|---|---|---|
| `handles` | string[] | **yes** | Entities to transform |
| `action` | enum | no | `move` (default), `copy`, `rotate`, `scale`, `mirror`, `array` |
| `vector` | Point | move/copy | Displacement `[dx,dy]` or `[dx,dy,dz]` |
| `center` | Point | rotate/scale | Base point of the rotation / scaling |
| `angle_deg` | number | rotate | Degrees, counterclockwise positive |
| `factor` | number | **scale** | Non-zero scale factor (`scale` refuses without it) |
| `axis` | Point[2] | **mirror** | Mirror axis as `[[x1,y1],[x2,y2]]` (`mirror` refuses without it) |
| `copy` | boolean | mirror/copy | Keep the originals; `copy`/`mirror+copy` return the new handles in `result.created` |
| `rows`, `columns` | integer | array | 1–100 each, default 1 |
| `row_spacing`, `column_spacing` | number | array | Grid pitch in drawing units |

REST: **route** `POST /entities/transform`.

```json
{"op":"entities_transform","handles":["63"],"action":"rotate","center":[50,25],"angle_deg":90}
→ {"ok":true,"status":"completed","result":{"affected":1,"created":[]},…}

{"op":"entities_transform","handles":["63"],"action":"copy","vector":[0,-20]}
→ {"ok":true,"status":"completed","result":{"affected":1,"created":["68"]},…}
```

#### `entities_copy_to` — clone into another open document

| Field | Type | Required | Meaning |
|---|---|---|---|
| `handles` | string[] | **yes** | Entities of the *current active* document to copy |
| `target_document_id` | integer | **yes** | The tab that receives the copies |
| `document_id` | integer | deprecated | Old alias of `target_document_id`; one of the two is required |

Referenced layer definitions travel along. Result: `created[]` (new handles)
+ `count`. REST: **route** `POST /entities/copy-to`.

```json
{"op":"entities_copy_to","handles":["63","64"],"target_document_id":2}
→ {"ok":true,"status":"completed","result":{"document_id":2,"created":["1F","20"],"count":2},…}
```

#### `text_replace` — find & replace across text entities

| Field | Type | Required | Meaning |
|---|---|---|---|
| `find` | string | with `replace` | Text to find — exactly one of `find` / `pairs` is required |
| `replace` | string | no | Replacement; defaults to `""` (deletes the match) |
| `pairs` | `{find,replace}[]` | with `find` | Several pairs, applied in order |
| `match_case` | boolean | no | Case-sensitive search (default insensitive) |
| `whole_word` | boolean | no | Match whole words only |
| `ignore_accents` | boolean | no | Ignore diacritics (defaults to true when `match_case` is false) |
| `dry_run` | boolean | no | Report what would change — no mutation, no undo entry |
| `replace_all` | boolean | no | Replace every occurrence inside a matching entity (default true; false replaces the first) |
| `scope` | enum | no | `all` (default), `active_space`, `model_space`, `blocks` |
| `layer` | string | no | Layer name filter |
| `handles` | string[] | no | Handle filter |
| `type` | string | no | Entity type filter (TEXT, MTEXT, …) |
| `bounds` | number[4] | no | `[min_x,min_y,max_x,max_y]` filter |

One undo step for the whole run. Result: `replaced` (total occurrences
swapped), `entities_changed` (count) and `changes[]` (one `{handle, type,
before, after, replaced}` entry per touched entity — INSERT attribute edits
add `attribute_tag`); `"dry_run":true` returns the same report with nothing
mutated.

```json
{"op":"text_replace","find":"OLD","replace":"NEW"}
→ {"ok":true,"status":"completed","result":{"dry_run":false,"replaced":4,
   "entities_changed":3,"changes":[{"handle":"6A","type":"Text","before":"OLD-1","after":"NEW-1","replaced":1},…]},…}

{"op":"text_replace","pairs":[{"find":"Ø","replace":"DIA "},{"find":"mm","replace":""}],
 "scope":"model_space","dry_run":true}
→ {"ok":true,"status":"completed","result":{"dry_run":true,"replaced":12,"entities_changed":5,"changes":[…]},…}
```

### 6.4 Query and spatial filters — `query`

Parameters (all optional, combine freely — AND semantics):

| Parameter | Meaning |
|---|---|
| `type`, `layer` | Entity type name / layer name |
| `handles` | Exact handle list |
| `detail` | `summary` (identity + common), `geometry` (+ shape fields), `full` (+ complete serialized properties incl. bounds) |
| `offset`, `limit` | Paging — default `limit` 1000, max 10000; see [Pagination] below |
| `fields` | Projection (property paths) |
| `bounds` | `[x0,y0,x1,y1]` world-window filter (entities intersecting the box) — for a "fully inside" test, compare each entity's own `bounds` client-side |
| `near` | `[x,y(,z)]` — rank by kernel-computed distance |
| `contains_point` | `[x,y]` — closed curves containing the point (kernel ray test) |
| `intersections` | `[h1,h2]` — exact intersection points of two planar curves (kernel) |
| `where` | Property filter list over serialized entities (below) |

`where` filters use JSON Pointer paths into the serialized entity
(`"/radius"`, `"/common/layer"`) with the operators `eq`, `ne`, `lt`, `lte`,
`gt`, `gte`, `contains`, `starts_with`, `ends_with`, `in`, `exists`,
`not_exists`. Filters are validated up front; a bad pointer or unknown
operator fails the query before any entity is inspected. On REST the same
filter rides `GET /entities?where=<json-encoded>`.

Text entities return the raw string in `value` plus a formatting-free
rendering in `text`; degenerate-width text bounds are widened with a
documented estimate (height × 0.8 × character count) so region filters stay
usable.

#### Pagination

Every paged read — `query` and `records` alike — answers with
`count` (total matches), `returned` (rows in this page) and
`next_offset`:

| Field | Meaning |
|---|---|
| `count` | Total matches for the filters, independent of paging |
| `returned` | Rows in this response (≤ `limit`) |
| `next_offset` | Offset of the next page, or **`null` when exhausted** |

Defaults and bounds: `offset` starts at 0; `limit` defaults to **1000** and
is clamped to **10000**. Rows are ordered by handle, ascending — handles
beyond the first 1000 are reachable **only** through paging, so a client
that never follows `next_offset` silently sees a prefix of the drawing.

Loop pattern (transport-neutral):

```json
{"op":"query","type":"Line","offset":0,"limit":1000}   → next_offset:1000
{"op":"query","type":"Line","offset":1000,"limit":1000} → next_offset:2000
…                                                       → next_offset:null
```

Over REST the same ride is `GET /entities?type=Line&offset=…&limit=…`.
Numeric query params are parsed as numbers by build `2026.46` and later;
earlier builds dropped them (always page one) — detect by checking that
`returned` matches the requested `limit`. A page whose `returned` is less
than `limit` with a non-null `next_offset` cannot occur.

REST: **route** `GET /entities` with every parameter as a query string
(`GET /entities/{handle}` is a shortcut for one full-detail entity;
`/entities` alone is the alias of `query`).

```json
{"op":"query","type":"Circle","layer":"FRAME","detail":"geometry"}
→ {"ok":true,"status":"completed","result":{
     "count":2,"returned":2,"next_offset":null,
     "entities":[{"handle":"64","type":"Circle","center":[50,25,0],"radius":10,
                  "common":{"layer":"FRAME","color":1,…}}]},…}
```

```sh
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" \
  "http://127.0.0.1:7931/api/v1/entities?type=Line&offset=0&limit=1000"
```

### 6.4.1 Reads catalogue

Every read is transport-neutral; the ones with dedicated REST routes are
marked **route** (§6.11). Reads need no envelope.

#### `records` — paged database records · **route** `GET /records?collection=&type=&offset=&limit=`

| Parameter | Meaning |
|---|---|
| `collection` | One collection name; omit for the manifest of collections |
| `type` | Filter records by their type name |
| `offset`, `limit` | Same paging as `query` (default 1000, max 10000) |

Response: `revision`, `geometry_revision`, `collection`, `count`,
`returned`, `next_offset`, `records[]`. Without `collection` the response is
the collection manifest instead.

```json
{"op":"records","collection":"block_records","limit":2}
→ {"ok":true,"collection":"block_records","count":14,"returned":2,"next_offset":2,
   "records":[{"name":"MARK","handle":"7A",…},…]}
```

#### `record_schema` — the writable-field registry

No `collection` → the manifest `[{name, mutable, root_type, identity_paths,…}]`.
With `collection` (and optional `type`) → the generated schema for that
collection: every field with its type, enum values, units, and the write
rules `set_properties` (§6.7) enforces. Generate client-side forms from it.

```json
{"op":"record_schema","collection":"layers"}
→ {"ok":true,"name":"layers","mutable":true,"root_type":"Layer",
   "identity_paths":["/name"],"fields":[{"/name":…,"/color/index":…,"/is_locked":…},…]}
```

#### `layers` — layer table · **route** `GET /layers`

No parameters (plus the standard `offset`/`limit` paging). Response:
`current` (the active layer), `count`, `next_offset`, and `layers[]` — each
`{name, off, frozen, locked, color?, rgb?}` (ACI index and/or `[r,g,b]`).

```json
{"op":"layers"}
→ {"ok":true,"current":"0","count":3,"next_offset":null,
   "layers":[{"name":"0","off":false,"frozen":false,"locked":false,"color":7},
             {"name":"FRAME","off":false,"frozen":false,"locked":false,"color":1}]}
```

#### `header` — document header variables · **route** `GET /header`

No parameters. Returns `current_layer`, `current_text_style`,
`insertion_units`, `pdmode`, `pdsize`, `ltscale`, `annotation_scale_value`.

```json
{"op":"header"}
→ {"ok":true,"current_layer":"0","current_text_style":"Standard","insertion_units":4,
   "pdmode":35,"pdsize":0.0,"ltscale":1.0,"annotation_scale_value":1.0}
```

#### `properties` — the properties-panel contents of the selection

No parameters; reads the current selection. Response: `sections[]`, each
`{title, properties[]}` — the same rows the Properties panel shows.

```json
{"op":"properties"}
→ {"ok":true,"sections":[{"title":"Geometry","properties":[{"name":"Radius","value":"10"},…]},…]}
```

#### `measure` — kernel measurements of listed handles

| Field | Type | Required | Meaning |
|---|---|---|---|
| `handles` | string[] | no | Hex handles; **omit to measure the current selection** |

Response: `document_id`, `geometry_revision`, `measurements[]` — one per
handle: `bounds` (`min`/`max`), `curve` (`bounded`, `closed`, `length`,
`area` for closed planar curves) and, for solids, `mesh` metrics
(`vertices`, `triangles`, `surface_area`, `volume`, `centroid`,
`moment_of_inertia`, `principal_directions`, `principal_moments`,
`product_of_inertia`, `radii_of_gyration`).

```json
{"op":"measure","handles":["64"]}
→ {"ok":true,"document_id":1,"geometry_revision":9,"measurements":[
    {"handle":"64","type":"Circle","bounds":{"min":[40,15,0],"max":[60,35,0]},
     "curve":{"bounded":true,"closed":true,"length":62.83,"area":314.16}}]}
```

#### `snap` — what object snap a cursor point would get

| Field | Type | Required | Meaning |
|---|---|---|---|
| `point` | Point | **yes** | World point to probe |
| `from` | Point | no | Base point for perpendicular/tangent snaps |

Response: `snap` (name or null) and the snapped `world` point.

```json
{"op":"snap","point":[51,26]}
→ {"ok":true,"snap":"Center","world":[50,25,0]}
```

#### `grips` — the grips of the current selection

No parameters. Response: `grips[]` — `{handle, grip_id, point, shape,
moves_entity, axis?}`. The ids feed `grip_drag` (§6.13).

```json
{"op":"grips"}
→ {"ok":true,"grips":[{"handle":"63","grip_id":0,"point":[0,0,0],"shape":"Square","moves_entity":false}]}
```

#### `xdata_get` — read extended data · **route** `GET /entities/{h}/xdata[?app=]`

| Field | Type | Required | Meaning |
|---|---|---|---|
| `handles` | string[] | **yes** | Entities to read |
| `app` | string | no | One application name; omit for every application |

```json
{"op":"xdata_get","handles":["64"],"app":"SPEC"}
→ {"ok":true,"records":[{"handle":"64","app":"SPEC","data":[{"code":1000,"value":"PAGE-01"}]}]}
```

#### `get_selection` — the current selection, query-formatted

No parameters (the read-back half of `user_select`, §6.12). Response:
`entities[]` with `handle`, `type`, `display_type`, `layer`, `bounds`,
`text`/`value` for text entities, `block`/`position` for inserts, and the
full serialized properties. REST: **route** `POST /get_selection`.

```json
{"op":"get_selection"}
→ {"ok":true,"count":1,"entities":[{"handle":"6F","type":"Block Reference",
    "display_type":"Block Reference","block":"ANCHOR_A","position":[0,0,0],…}]}
```

#### `text_search` — find text without changing it

Same filter fields as `text_replace` (§6.3: `find` required; `match_case`,
`whole_word`, `ignore_accents`, `scope`, `layer`, `handles`, `type`,
`bounds`) plus `limit` (default 500, max 5000) and `document_id`. Response:
the matching entities with their text values and locations.

```json
{"op":"text_search","find":"PAGE-"}
→ {"ok":true,"count":3,"matches":[{"handle":"66","type":"Text","value":"PAGE-01","position":[10,50,0]},…]}
```

#### `text_audit` — text-quality report for the drawing

Same filters as `text_search`; returns problem entries (missing styles,
placeholder text, encoding risks) with suggestions.

```json
{"op":"text_audit"}
→ {"ok":true,"suspects":[{"handle":"71","type":"MText","issue":"missing_style","suggestions":["Standard"]},…]}
```

#### `events` — the event ring

| Field | Type | Required | Meaning |
|---|---|---|---|
| `cursor` | integer | no | Last sequence seen; the response carries everything after it |

Response: `cursor` (the new position), `resync` (true when the ring wrapped
and the client should refresh `state`), `events[]` — command lifecycle,
document and selection changes, 128-entry ring, polling by design.

```json
{"op":"events","cursor":41}
→ {"ok":true,"cursor":43,"resync":false,"events":[{"sequence":42,"kind":"document",…},{…}]}
```

#### `history` — the command line trail · REST `POST /history`

No parameters. Response: `entries[]` of `{kind, text}` — every command the
person *and* every client ran, oldest first.

```json
{"op":"history"}
→ {"ok":true,"entries":[{"kind":"Client","text":"entities_create"},{"kind":"Command","text":"LINE 0,0 10,10"}]}
```

#### `capture` — PNG snapshot of the viewport or window

| Field | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | **yes** | Destination PNG path |
| `scope` | enum | no | `viewport` (default) or `window` |
| `max_dimension` | integer | no | Longest-edge pixel cap, 256–4096, default 1600 |
| `highlight_handles` | string[] | no | Select these handles before the shot |
| `focus_handles` / `bounds` / `view` | string[] / number[4] / enum | no | Frame the shot: zoom to handles, to a world window, or `extents`/`selection` |

GUI sessions only (headless answers `gui_required`). Response carries
`path`, `scope`, `width`, `height`, the camera state and — grounding for
vision clients — `pixel_to_world_matrix`, `world_to_pixel_matrix` and
`viewport_world_bounds`.

```json
{"op":"capture","path":"C:/out/shot.png","view":"extents","max_dimension":2048}
→ {"ok":true,"status":"completed","result":{"path":"C:/out/shot.png","scope":"window",
   "width":2048,"height":1158,"pixel_to_world_matrix":[…],…},…}
```

#### `audit` — pre-save structural report

| Field | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | no | Audit against this save target instead of the current file |
| `target_format`, `target_version` | enum | no | The delivery target (same values as `save`) |

Returns the structural findings (`save_verified` runs the same audit as part
of its roundtrip): undeclared layers, dangling references, lossy records.

```json
{"op":"audit","target_format":"dwg","target_version":"R14"}
→ {"ok":true,"audit":{"lossy_records":[],"undeclared_layers":[],"dangling_references":[]}}
```

MCP `ocs_read` additionally exposes `entity_schema` (§6.2), `validate`
(preflight a request against the schema) and `guide` (on-demand guides:
`workflow`, `architecture`, `capture`, `batch-bindings`).

### 6.5 Blocks

#### `block_define` — wrap entities into a named block

| Field | Type | Required | Meaning |
|---|---|---|---|
| `name` | string | **yes** | Block name (unique, `*` prefix reserved) |
| `handles` | string[] | **yes** | The entities that form the definition — they move into it (flattened at `base` = block origin) |
| `base` | Point | **yes** | The block's origin point in world coordinates |
| `insert_at` | Point | no | Placement of the replacement INSERT (defaults to `base`, so the drawing looks unchanged) |
| `replace` | boolean | no | Drop a same-named definition — children, markers and inserts — inside the same undo step before creating the new one (idempotent re-import) |

Result: `{block, insert}` (the new INSERT's handle). REST: **route**
`POST /blocks`.

```json
{"op":"block_define","name":"MARK","base":[0,0,0],"handles":["63","64"]}
→ {"ok":true,"status":"completed","result":{"block":"MARK","insert":"69"},…}
```

#### `block_delete` — remove a block definition

| Field | Type | Required | Meaning |
|---|---|---|---|
| `name` | string | **yes** | The definition to remove — completely: child entities, block markers, table record, one undo step (`Delete block <name>`) |
| `force` | boolean | no | A referenced block is refused (`block_referenced`); `true` erases the referencing INSERTs in the same step and reports how many |

Result: `{block, erased, references}`. Unknown name → `block_missing`;
layout/anonymous/xref/dynamic records → `block_protected`; open BEDIT session
→ `block_in_edit`; invalid/taken names → `block_invalid_name`/`block_exists`.
REST: **route** `DELETE /blocks/{name}` (`?force=true` to override references).

```json
{"op":"block_delete","name":"MARK","force":true}
→ {"ok":true,"status":"completed","result":{"block":"MARK","erased":5,"references":1},…}
```

#### `wblock` — export a standalone drawing

| Field | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | **yes** | Target `.dwg`/`.dxf` (extension decides format) |
| `handles` | string[] | one of | Entities to export — `handles` or `block` is required |
| `block` | string | one of | A whole block definition to export |
| `template` | string | no | A base file whose tables/styles the export inherits |
| `normalize` | boolean | no | Shift the export so its overall bounds minimum lands on the origin |

The source document is untouched. Result: `{path, entities, normalized}`.
REST: **route** `POST /wblock`.

```json
{"op":"wblock","path":"C:/out/page-01.dwg","handles":["63","64"],"normalize":true}
→ {"ok":true,"status":"completed","result":{"path":"C:/out/page-01.dwg","entities":2,"normalized":true},…}
```

### 6.6 Layouts and plotting

#### `layout_create` — add a paper-space layout

| Field | Type | Required | Meaning |
|---|---|---|---|
| `name` | string | **yes** | New layout tab name; duplicates refused (`layout_exists`) |

The layout starts with the default page setup and a sheet viewport.
REST: **route** `POST /layouts`.

```json
{"op":"layout_create","name":"SHEET-01"}
```

#### `page_setup_set` — store plot configuration on a layout

| Field | Type | Required | Meaning |
|---|---|---|---|
| `layout` | string | **yes** | Target layout |
| `paper` | string | no | Canonical paper catalog name, e.g. `"ISO_A4_(210.00_x_297.00_MM)"` |
| `orientation` | enum | no | `Portrait` / `Landscape` |
| `fit` | boolean | no | Fit drawing to sheet (default true) |
| `scale` | string | no | `"paper:drawing"` ratio like `"1:100"` — overrides `fit` |
| `center` | boolean | no | Center on sheet (default true) |
| `offset_x`, `offset_y` | number | no | Plot offsets in mm |
| `plot_style` | string | no | CTB path or named plot style table |
| `area` | enum | no | `extents`/`display`/`limits`/`layout`/`window` |
| `window` | number[4] | **when area=window** | `[x0,y0,x1,y1]` |

Explicit fields win; the rest keeps the stored setup — and the stored setup
is what `plot` consumes by default. REST: **route**
`PUT /layouts/{name}/page-setup`.

```json
{"op":"page_setup_set","layout":"SHEET-01","paper":"ISO_A4_(210.00_x_297.00_MM)",
 "orientation":"Landscape","scale":"1:100"}
```

#### `plot` — write PDF

| Field | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | **yes** | Output path, must end `.pdf` |
| `layout` | string | no | `"Model"` (default), a layout name, or `"all"` |
| `area` | enum | no | `extents` (default), `display`, `limits`, `layout`, `window` |
| `window` | number[4] | **when area=window** | `[x0,y0,x1,y1]` |
| `paper`, `orientation`, `center`, `offset_x`, `offset_y`, `plot_style` | — | no | Same meanings as `page_setup_set`; omitted fields fall back to the stored setup |
| `fit` / `scale` | bool / string | no | `fit` defaults to true when `scale` is omitted; a `"1:100"`-style ratio overrides it |
| `upside_down` | boolean | no | Rotate content 180° |
| `transparency`, `lineweights`, `merge_lines`, `stamp` | boolean | no | Output toggles |
| `per_page` | boolean | no | With `layout:"all"`: **one PDF per layout**, named `<stem>-<Layout>.pdf` |

PDF only, synchronous, no device dialogs. Result: `{path, pages,
page_sizes}` (paper sizes in mm); with `per_page` → `{files, pages}`.
REST: **route** `POST /plot`.

```json
{"op":"plot","path":"C:/out/pages.pdf","layout":"all","per_page":true}
→ {"ok":true,"status":"completed","result":{"files":["C:/out/pages-Model.pdf","C:/out/pages-SHEET-01.pdf"],"pages":2},…}
```

```sh
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST \
  http://127.0.0.1:7931/api/v1/plot -H "Content-Type: application/json" \
  -d '{"path":"C:/out/plan.pdf","area":"extents","scale":"1:100"}'
```

### 6.7 Metadata and variables

**Extended data (`xdata_set` / `xdata_get`)** — typed key-value records per
application name on any entity; the application table entry is registered
implicitly on write. An empty (or absent) `data` list removes the record.
Result of a write: `{updated, app}`. REST: **route**
`PUT /entities/{handle}/xdata/{app}` (body = the data array) and
`DELETE /entities/{handle}/xdata/{app}` to clear.

```json
{"op":"xdata_set","handles":["65"],"app":"SPEC","data":[{"code":1000,"value":"PAGE-01"},{"code":1070,"value":3}]}
→ {"ok":true,"status":"completed","result":{"updated":1,"app":"SPEC"},…}
```

| Code | Value type |
|---|---|
| `1000` | string |
| `1003` | layer name |
| `1004` | hex bytes |
| `1005` | hex handle |
| `1010`–`1013` | `[x,y,z]` point / position / displacement / direction |
| `1040` / `1041` / `1042` | real |
| `1070` / `1071` | 16-bit / 32-bit integer |

**System variables (`sysvar`)** — atomic get/set over a fixed registry:

| Var | Meaning | Writable |
|---|---|---|
| `ltscale`, `celtscale` | global / current-entity linetype scale | ✓ |
| `clayer`, `ctextstyle` | current layer / text style (must exist) | ✓ |
| `textsize`, `filletrad`, `pdmode`, `pdsize` | defaults for new content | ✓ |
| `mirrtext` (`0`/`1`), `insunits`, `osmode` | behaviour flags | ✓ |
| `extmin`, `extmax` | model-space extents `[x,y,z]` | read-only |

`{"op":"sysvar","set":{"ltscale":2.5}}` applies the whole object atomically
(`unknown_sysvar` / `invalid_sysvar_value` refuse it all);
`{"op":"sysvar","get":["ltscale"]}` → `result.values`. Omitted `get` returns
every readable name. REST: **routes** `GET /sysvars?names=ltscale,clayer` ·
`POST /sysvars`.

```json
{"op":"sysvar","get":["ltscale"]}
→ {"ok":true,"status":"completed","result":{"values":{"ltscale":2.5}},…}
```

**Record editing (`set_properties`)** — typed, validated property writes on
any database record via RFC 6901 JSON Pointer paths, with compare-and-set
support (`expected` on an update refuses when the current value differs);
`records`/`record_schema` describe every writable field.

```json
{"op":"set_properties","collection":"entities","handle":"64",
 "updates":[{"path":"/common/layer","value":"WALLS"},
            {"path":"/radius","value":12,"expected":10}]}
→ {"ok":true,"status":"completed",
   "result":{"collection":"entities","handle":"64","name":"Circle","paths":["/common/layer","/radius"],"changed":2},…}
```

### 6.8 Groups and selection sets

#### `group_create` — a named group over existing entities

| Field | Type | Required | Meaning |
|---|---|---|---|
| `name` | string | **yes** | Group name |
| `handles` | string[] | **yes** | Member entities (must exist) |

Result: `{group, handle}` — the group's own database object handle.
REST: **route** `POST /groups`.

```json
{"op":"group_create","name":"FASTENERS","handles":["6F","70"]}
→ {"ok":true,"status":"completed","result":{"group":"FASTENERS","handle":"88"},…}
```

#### `selection_set_save` — store a named handle set

| Field | Type | Required | Meaning |
|---|---|---|---|
| `name` | string | **yes** | Set name (per session) |
| `handles` | string[] | **yes** | Entities to remember |

Result: `{name, handles, count}`. REST: **route** `POST /selection-sets`.

```json
{"op":"selection_set_save","name":"TITLE-BLOCKS","handles":["6F","70"]}
→ {"ok":true,"status":"completed","result":{"name":"TITLE-BLOCKS","handles":["6F","70"],"count":2},…}
```

#### `selection_set_load` — recall a saved set

| Field | Type | Required | Meaning |
|---|---|---|---|
| `name` | string | **yes** | Saved set name (unknown → `selection_set_missing`) |
| `select` | boolean | no | Also make it the current selection (default true) |

Result: `{name, handles, selected}`. REST: **route**
`GET /selection-sets/{name}?select=true`.

```json
{"op":"selection_set_load","name":"TITLE-BLOCKS"}
→ {"ok":true,"status":"completed","result":{"name":"TITLE-BLOCKS","handles":["6F","70"],"selected":2},…}
```

### 6.9 Images — `embed_image`

| Field | Type | Required | Meaning |
|---|---|---|---|
| `path` | string | **yes** | Image file (png/jpg) |
| `at` | Point | no | Lower-left corner in world coordinates |
| `width` | number | no | World width in drawing units |
| `linked` | boolean | no | Store as path-linked `RasterImage` instead of embedded bytes |
| `calibrate` | object | no | Automatic 2-point reference calibration: `{point_a:[px1,py1], point_b:[px2,py2], distance:<world units>, align_to:[x,y]}` |
| `layer` | string | no | Target layer (auto-created when missing, e.g. `_XREF`) |
| `lock_layer` | boolean | no | Lock the target layer after attaching |
| `source_points` / `target_points` | Point[2] | no | Explicit 2-point alignment: image pixel points ↔ CAD world points |

| Mode | Behaviour |
|---|---|
| **Embedded (default)** | The bytes are stored inside the drawing as a self-contained frame entity. There is no external path, so the image cannot break when the source file is moved or deleted, and it travels through block definitions, clones and exports like any entity. Trade-offs: the DWG grows by the image size, and the picture never updates when the source file changes. Use this for barcodes/QR and any image that is part of the deliverable. |
| `"linked":true` | Stores a path-referenced image. The file must travel with the drawing and *can* break; the mode exists for images that must be swappable without touching the drawing. |

```json
{"op":"embed_image","path":"C:/img/qrcode.png","at":[180,10],"width":20}
→ {"ok":true,"status":"completed",
   "result":{"handle":"8C","kind":"RasterImage","path":"C:/img/qrcode.png",
             "at":[180,10,0],"width":20,"height":20,"scale":1.0,"linked":false},…}
```

Calibrated underlay — scale and place a scanned plan so two known pixels sit
exactly 6500 drawing units apart, aligned to the world origin:

```json
{"op":"embed_image","path":"C:/scan/plan.png","linked":true,
 "calibrate":{"point_a":[52,910],"point_b":[450,910],"distance":6500,"align_to":[0,0]},
 "layer":"_XREF","lock_layer":true}
```

Repairing a broken linked image from a drawing authored elsewhere — no
dedicated op needed:

1. `query` with `type:"RasterImage"`, `detail:"full"` — the entity exposes
   its `file_path` and world `bounds`.
2. `entities_delete` the broken reference by handle.
3. `embed_image` a replacement at the same `at`/`width` derived from the old
   bounds — embedded, so it cannot break again.

### 6.10 Commands, batches, control

#### `run` — execute one command line headlessly

| Field | Type | Required | Meaning |
|---|---|---|---|
| `cmd` | string | **yes** | Command name + space-separated prompt answers (`LINE 0,0 10,10`); points `x,y[,z]`, options by their displayed token |

Requires an open document. Response `status` is `completed` or
`waiting_input` (the command wants more — feed it with `input` or press on
with `cancel`). REST: **route** `POST /commands` with `{"cmd":…}`.

```json
{"op":"run","cmd":"LINE 0,0 10,10"}
→ {"ok":true,"status":"completed","result":{…},…}
```

#### `start` / `input` — interactive command dialog

`start` opens a command and leaves it pending; `state.command` then reports
`accepts[]`, `options[]` and `input_example`. `input` feeds one step:

| Field | Type | Required | Meaning |
|---|---|---|---|
| `kind` | enum | **yes** | `text`, `token`, `point`, `entity`, `structure`, `selection`, `enter` |
| `text` | string | when kind=`token` | Free text or the token option |
| `point` | Point | when kind=`point` | The point to feed |
| `space` | enum | no | `wcs` (default), `ucs`, `relative` |
| `handle` | string | when kind=`entity`/`structure` | Target entity (with `point` as the pick position) |

```json
{"op":"start","cmd":"CIRCLE"}
{"op":"input","kind":"point","point":[10,20]}
{"op":"input","kind":"token","text":"D"}
{"op":"input","kind":"text","text":"25"}
```

#### `cancel` — Escape

No fields. Aborts the running command — or retracts this client's own
pending pick (§6.12).

```json
{"op":"cancel"}
```

#### `batch` — sequential ops in one call

| Field | Type | Required | Meaning |
|---|---|---|---|
| `steps` | op[] | **yes** | 1–64 operations; each step sees the previous step's state; stops at the first failure and reports `completed_steps` / `next_step` |

```json
{"op":"batch","steps":[
  {"op":"entities_create","entities":[{"type":"Line","start":[0,0],"end":[10,0]}]},
  {"op":"block_define","name":"TICK","base":[0,0],"handles":["70"]}
]}
→ {"ok":true,"status":"completed","result":{"completed_steps":2},…}
```

#### `select` — change the current selection

| Field | Type | Required | Meaning |
|---|---|---|---|
| `handles` | string[] | no | Explicit handles to select |
| `type` / `layer` | string | no | Entity type / layer filter |
| `window` | number[4] | no | World box `[x1,y1,x2,y2]`: selects what lies inside it, as a viewport window does |
| `crossing` | boolean | no | With `window`: also select what the box crosses |
| `clear` | boolean | no | Clear the existing selection first |
| `where` | filter[] | no | Same predicates as `query`'s `where` (§6.4) |

```json
{"op":"select","layer":"FRAME","clear":true}
```

#### `property` — set one property on the selected entities

| Field | Type | Required | Meaning |
|---|---|---|---|
| `field` | string | **yes** | Property name (`color`, `layer`, `linetype`, …) |
| `value` | any | **yes** | New value (§2 conventions — color is the ACI index) |

```json
{"op":"property","field":"color","value":1}
```

#### `view_focus` — zoom to entities and highlight them

| Field | Type | Required | Meaning |
|---|---|---|---|
| `handles` | string[] | **yes** | Entities to frame |
| `highlight` | boolean | no | Also select them (default true) |

GUI sessions; headless answers `gui_required`. Result: `{zoomed, selected}`.

```json
{"op":"view_focus","handles":["6F"]}
→ {"ok":true,"status":"completed","result":{"zoomed":true,"selected":1},…}
```

#### `action` — trigger a named UI action

| Field | Type | Required | Meaning |
|---|---|---|---|
| `name` | string | **yes** | A registered action name (grid/ortho/zoom toggles and dialogs — the enum is advertised in the MCP schema and refused with a name list when wrong) |

```json
{"op":"action","name":"zoom_extents"}
```

#### `operation` — poll a pending operation

| Field | Type | Required | Meaning |
|---|---|---|---|
| `request_id` | string | **yes** | The id of the op to poll (async work answers `status:"running"`, §5.1) |

```json
{"op":"operation","request_id":"rest-7"}
→ {"ok":true,"status":"completed","result":{…},…}
```

#### `stop` — the kill switch

No fields. Disables all automation **mutations** for the session until
someone re-enables automation in the desktop GUI; further mutations answer
`disabled`. Reads keep working. The point: an integrator that misbehaves can
shut itself off without killing the editor.

```json
{"op":"stop"}
→ {"ok":true,"status":"completed",…}
```

`events`, `history` and `capture` are reads — full contracts in §6.4.1.

### 6.11 Complete REST endpoint table

| Method & path | Purpose |
|---|---|
| `GET /ready` · `/state` · `/capabilities` · `/openapi` | Readiness · full state · feature advertisement · OpenAPI 3 document |
| `GET /documents` · `POST /documents` · `DELETE /documents/{id}?discard=` | List · open/template/new · close |
| `GET /sysvars?names=` · `POST /sysvars` | Read · set |
| `POST /layouts` · `PUT /layouts/{name}/page-setup` | Create · write plot configuration |
| `GET /entities` · `GET /entities/{h}` · `POST /entities` · `DELETE /entities?handles=` | Query (incl. `where`) · one entity · create · erase |
| `POST /entities/transform` · `POST /entities/copy-to` | Modify · cross-document copy |
| `GET/PUT/DELETE /entities/{h}/xdata[/{app}]` | Extended data |
| `POST /blocks` · `DELETE /blocks/{name}` | Define (+`replace`) · delete |
| `POST /groups` · `POST /selection-sets` · `GET /selection-sets/{name}` | Groups · selection sets |
| `POST /getpoint` · `POST /user_select` | Person-in-the-loop picks (§6.12; the connection parks) |
| `POST /plot` · `/wblock` · `/images` · `/commands` · `/undo` · `/redo` · `/save` | Publishing, export, command line, lifecycle |
| `GET /layers` · `/header` · `/records` | Database reads |
| `POST /file-identity` | Stable drawing GUID |
| `POST /{op}` | Passthrough for **every** op of §6 — mutations *and* reads, including `text_replace`, `save_verified`, `stop`, the viewport simulation of §6.13, `operation` and `cancel` |

### 6.12 Human-in-the-loop picks — `user_select` & `getpoint`

These two operations hand the screen to the person at the desk. They are
answerable only where a person exists: on the GUI-hosted channel (§3.2) or
MCP sessions attached to a desktop build, a headless session answers
`gui_required`. Both stay pending — pollers see `running` — until that person
answers, and both can be retracted programmatically with
`{"op":"cancel","request_id":"<a fresh id>"}`.

**`user_select`** — ssget-style sampling. The person picks entities with the
normal gestures (click, window, crossing), **Enter** confirms, **Escape**
cancels.

```json
{"protocol":1,"op":"user_select","request_id":"sample-1","document_id":1,
 "type":"INSERT","layer":"TITLE","prompt":"Chọn các block cần lấy props",
 "detail":"full","clear":true}
```

| Field | Meaning |
|---|---|
| `type`, `layer` | optional filters for what counts as picked; matching is case-insensitive against the display name (`"Block Reference"`) and the DXF name (`"INSERT"`) |
| `prompt` | optional; replaces the default hint on the command line |
| `detail` | `summary` / `geometry` / `full` (default) — `full` returns each entity's complete serialized properties |
| `clear` | default `true`: the request starts a fresh selection; `false` keeps whatever is selected |

Confirm resolves with:

```json
{"ok":true,"status":"completed","result":{
  "cancelled":false,"count":1,"ignored":1,
  "handles":["6F"],
  "entities":[{"handle":"6F","type":"Block Reference","block":"ANCHOR_A",
               "position":[0,0,0],"properties":{…}}]}}
```

`ignored` counts picks the `type`/`layer` filter deselected at confirm time
(the person's gestures are never blocked; only the answer is filtered).
Escape — or a `cancel` retraction, or the document closing — resolves with
`status:"cancelled"` and `{"cancelled":true,"count":0}`. Starting a pick
while a command or dialog is active is refused (`user_input_busy`); a second
concurrent pick is refused (`interactive_pending`).

**`getpoint`** — one snapped world point from the next left-click.

```json
{"protocol":1,"op":"getpoint","request_id":"qr-spot","document_id":1,
 "prompt":"Chọn vị trí đặt QR code"}
→ {"ok":true,"status":"completed","result":{"point":[125.5,64.25,0.0]}}
```

Escape cancels with `{"cancelled":true}`. `detail` does not apply.

**What the person sees while a pick is pending** (so integrators know the
request is visible even before anyone clicks):

- the command line pins a labeled, non-fading request line:
  `[user_select · sample-1] Chọn các block cần lấy props  (Enter confirms, Esc cancels)`;
- the automation pill on the command line turns blue — *"MCP is waiting for
  you to pick — Enter confirms, Esc cancels"*;
- the viewport cursor trades its crosshair arms for a blue pickbox;
- the answer is printed back into the message list with the same identity
  (`user_select sample-1: handed 1 object(s) to the client.`), and the full
  trail of every client operation — including failures — stays reviewable in
  the command-line history.

On the GUI-hosted HTTP channel (§3.2) the whole exchange rides one parked
connection: the request holds until the person answers (up to 30 minutes,
then `response_timeout`), so no polling loop is needed. Callers that prefer
polling may treat an `accepted` reply as the answer and follow the operation
with `{"op":"operation","request_id":…}` reads; a parked pick that loses its
connection (or its caller) is retracted automatically, and the server-side
trail of every state change — start, answer, cancel — lands on stderr as
`[pick] <op> <request_id>: …` lines for cross-referencing against the
client's own log.

---

### 6.13 Viewport simulation — `click`, `pointer`, `grip_drag`

These three ops drive the viewport the way the mouse does, for clients that
must reproduce a person's gestures rather than edit the database directly
(interactive commands that expect picks, grip stretches, pan/zoom sequences).
They work on any session with a viewport; each step reports what the cursor
sees. All three ride the REST passthrough (`POST /api/v1/{op}`).

#### `click` — one left-click at a world point

| Field | Type | Required | Meaning |
|---|---|---|---|
| `point` | Point | **yes** | World point to click on (projected through the live camera) |
| `modifiers` | string | no | Keys held during the click: `"ctrl"`, `"shift"` or `"ctrl+shift"` (Ctrl+click picks the solid of a composite under the cursor) |

```json
{"op":"click","point":[10,0,0],"modifiers":"ctrl"}
```

#### `pointer` — replay a mouse sequence

| Field | Type | Required | Meaning |
|---|---|---|---|
| `events` | event[] | **yes** | In order; each holds exactly one of `move` (canvas pixels `[x,y]`), `move_world` (world `[x,y,z]`, projected through the camera the cursor edits with), `press` / `release` (`"left"`, `"right"`, `"middle"`), `wheel` (lines, positive zooms in) |
| `modifiers` | string | no | Keys held throughout the sequence |

A press→move→release run goes through the same cursor path as a hand drag,
so rubber-band windows and grip drags behave identically. Each step reports
the cursor's world point.

```json
{"op":"pointer","events":[
  {"move_world":[10,0,0]},{"press":"left"},
  {"move_world":[12,3,0]},
  {"release":"left"}
]}
```

#### `grip_drag` — move one grip to a world point

| Field | Type | Required | Meaning |
|---|---|---|---|
| `handle` | string | **yes** | Entity whose grip moves |
| `grip_id` | integer | **yes** | Grip id from the `grips` read (§6.4.1) |
| `point` | Point | **yes** | World point the grip moves to |

Reads the way a grip pick-and-place does, with object snaps, grid, ortho,
polar and tracking off for the move — the grip lands exactly on `point`.

```json
{"op":"grip_drag","handle":"2A","grip_id":0,"point":[10,0,0]}
```

---

## 7. Worked examples

### 7.1 Draw, verify, publish (REST)

```sh
# Create (missing layers auto-created; the batch is atomic).
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/entities -H "Content-Type: application/json" -d '{
  "entities":[
    {"type":"Line","start":[0,0],"end":[100,0],"layer":"FRAME"},
    {"type":"Text","value":"PAGE-01","position":[10,50],"height":3.0}
  ]}'
# → 201 {"result":{"handles":["63","64"],"created":2,"layers_created":["FRAME"]}}

# Filter by property.
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" "http://127.0.0.1:8090/api/v1/entities?type=Line&where=%5B%7B%22path%22%3A%22%2Flayer%22%2C%22op%22%3A%22eq%22%2C%22value%22%3A%22FRAME%22%7D%5D"

# Mark, wrap into a reusable block, plot every sheet, save as a template.
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X PUT http://127.0.0.1:8090/api/v1/entities/64/xdata/SPEC \
  -H "Content-Type: application/json" -d '[{"code":1000,"value":"PAGE-01"}]'
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/blocks \
  -H "Content-Type: application/json" -d '{"name":"MARK","base":[0,0,0],"handles":["63","64"]}'
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/plot \
  -H "Content-Type: application/json" -d '{"path":"C:/out/pages.pdf","layout":"all","per_page":true}'
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/save \
  -H "Content-Type: application/json" -d '{"path":"C:/out/session.dwt"}'
```

### 7.2 Sample entities with the person at the screen (GUI-hosted)

```sh
OpenCADStudio.exe "C:/drawings/plan.dwg" --http 8090   # the editor stays interactive

# Ask the person to pick block references; the connection parks until they
# press Enter (or Esc). No polling loop needed.
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -m 1800 -X POST http://127.0.0.1:8090/api/v1/user_select \
  -H "Content-Type: application/json" \
  -d '{"request_id":"sample-1","type":"INSERT","detail":"full","prompt":"Pick the title blocks"}'
# → {"ok":true,"status":"completed","result":{"cancelled":false,"count":2,…}}

# Read the same entities back, then continue driving the live drawing.
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/get_selection -d '{}'
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/entities/transform \
  -H "Content-Type: application/json" \
  -d '{"handles":["6F"],"action":"move","vector":[5,0]}'
```

### 7.3 Clone a region into a new, normalized drawing

```json
{"protocol":1,"op":"wblock","request_id":"clone-1","document_id":1,
 "handles":["63","64"],
 "template":"C:/tpl/base.dwt",
 "normalize":true,
 "path":"C:/out/page-01.dwg"}
```

The export inherits the template's tables/styles and is shifted so its
minimum lands on the origin. Open it afterwards with `{"op":"open","path":…}`
and verify with `query`.

### 7.4 Stable identity + idempotent retry

```json
{"protocol":1,"op":"file_identity","request_id":"fid-1","document_id":1}
→ {"ok":true,"result":{"identity":"5db78b90-ca0b-4a4f-ad68-8c860483a41f","created":true}}
{"protocol":1,"op":"file_identity","request_id":"fid-1","document_id":1}
→ same response served from the idempotency cache — never executed twice
```

---

## 8. Surface status and upstreaming map

This spec documents the fork's surface, which is a strict superset of
upstream `main`. Everything in the **core** column exists upstream today and
is documented here unchanged; each **piece** row is a self-contained,
independently reviewable addition — the matching section of this spec is
meant to travel with its piece's PR, so no section ever runs ahead of the
code it describes.

| Scope | Contents | Spec sections |
|---|---|---|
| **Core (upstream `main`)** | Transports `--serve` (stdio JSONL), `--serve --port` (TCP), `--mcp`; ops `new`, `open`, `save`, `save_verified`, `activate`, `switch_document`, `select`, `run`, `start`, `input`, `cancel`, `stop`, `action`, `property`, `set_properties`, `text_replace`, `embed_image`, `capture`, `undo`, `redo`, `grip_drag`, `click`, `pointer`; reads `state`, `hello`, `capabilities`, `query`/`entities`, `records`, `record_schema`, `entity_schema`, `layers`, `header`, `properties`, `measure`, `snap`, `grips`, `xdata_get`, `get_selection`, `text_search`, `text_audit`, `audit`, `events`, `history`; envelope conventions and the core error codes | §1–§5, §6.1, §6.4, §6.10, §6.13 |
| Piece A — entity CRUD | `entities_create`, `entities_delete`, `entities_transform`, `entities_copy_to` | §6.2, §6.3 |
| Piece B — blocks & export | `block_define`, `block_delete`, `wblock` | §6.5 |
| Piece C — publishing | `layout_create`, `page_setup_set`, `plot` (incl. `per_page`) | §6.6 |
| Piece D — metadata | `sysvar`, `xdata_set`/`xdata_get`, `file_identity` | §6.7 |
| Piece E — collections | `group_create`, `selection_set_save`/`load` | §6.8 |
| Piece F — control plane | `batch`, `events`, `view_focus`, `close`, `where` filters | §6.4 (`where`), §6.10 |
| Piece G — REST transport | `--http` headless server, `/api/v1` routes, OpenAPI 3 document | §3.1, §6.11, §7.1 |
| Piece H — person-in-the-loop | GUI-hosted `--http` channel, `user_select`, `getpoint`, the on-screen pending signals | §3.2, §6.12, §7.2 |

Sections §3.2/§6.12 (piece H) additionally require the desktop build; over a
headless session every documented request still answers truthfully
(`gui_required`), so a client written against this spec behaves consistently
on both.

---

## 9. Versioning and compatibility

- The wire protocol version is **1**; it is declared in every envelope and in
  the greeting line. Additions (new ops, new optional fields, new error
  codes) are non-breaking; clients should ignore unknown fields and treat
  unknown op names in `capabilities` as "not available in this build".
- Feature detection: read `capabilities` before unfamiliar work — every op in
  this document is listed there when the running build supports it.
- Drawing files: DWG and DXF readers/writers are version-tolerant; templates
  (`*.dwt`) are byte-compatible DWG files.

*Companion documents: the automation walkthrough and MCP set-up
([`README.md`](README.md)) and the OpenAPI 3 description served at
`GET /api/v1/openapi`.*
