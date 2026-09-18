# OpenCAD Automation API — AutoCAD .NET parity catalog

Design target: an automation API surface equivalent in capability to the
AutoCAD .NET/ObjectARX developer API, so that clients written against the
conventions of the [Managed .NET Developer's Guide](https://help.autodesk.com/view/OARX/2022/ENU/)
(components page: [AcCoreMgd/AcDbMgd/AcMgd/AcCui](https://help.autodesk.com/view/OARX/2022/ENU/?guid=GUID-8657D153-0120-4881-A3C8-E00ED139E0D3))
can be ported to OpenCAD Studio without redesign.

Input documents:

- Autodesk: *Managed .NET Developer's Guide* chapters — Introduction,
  Interacting with the Application, Working with Objects, Database, Editor,
  Events, User Interface, Layouts/Plotting/Printing, Extending the User
  Interface.
- Team catalog: `INS.MCC.Client/docs/AutoCad-Api-Operations-Catalog.md`
  (SPM.ACAD port spec — sections referenced below as "Catalog §n").

All new operations ride the existing protocol-1 envelope (`protocol`, unique
`request_id`, `document_id`, optimistic `revision`/`geometry_revision`
preconditions, idempotent replay) described in `docs/automation/README.md`,
so every transport (stdio `--serve`, TCP, MCP, wasm) gains them at once.
"Exists" below means the operation is shipped today; "Proposed" entries are
the work items, prioritized **P0** (clients blocked today), **P1** (parity
completeness), **P2** (niche or deliberately out of scope).

## 1. Application and documents (AcMgd `ApplicationServices`)

| .NET API | OpenCAD status |
|---|---|
| `Application.DocumentManager` (MDI), `Document`, `MdiActiveDocument` | Exists — `{"op":"state"}` returns `documents[]` + `document_id`; `new`/`open`/`activate` switch tabs. |
| `Document.LockDocument()` | N/A — single-writer dispatcher with request idempotency replaces it. |
| `Document.SendStringToExecute` | Exists — `run` / `start` + `input`. |
| `Document.Close`, `DocumentCollection.CloseAll` | **Proposed P1** — `{"op":"close","document_id":N,"discard":true}`; today a document can only be replaced, never closed. |
| `Application.GetSystemVariable / SetSystemVariable` | Partial — `header` op exposes a fixed set (current layer/style, ltscale, pdmode, annotation scale). **Proposed P1** — `{"op":"sysvar","names":[...]}` / `{"op":"sysvar","name":"...","value":...}` over a generated registry of supported variables. |
| File identity (`SPMDOCUNIQUE`, Catalog §1.6) | Partial — header/summary records are readable; **Proposed P1** — stable `file_identity` (GUID) surfaced in `state` and stored per document. |

## 2. Working with objects and the database (AcDbMgd) — the core gap

### 2.1 Read (parity achieved)

| .NET API | OpenCAD status |
|---|---|
| `Transaction.GetObject(ForRead)` over every DB object | Exists — `records` (full serializable state, paged, filterable), `record_schema` (complete generated type registry, RFC 6901 paths, write rules), `entities`, `layers`, `header`. |
| `Entity.GeometricExtents` | Exists — `query detail:"full"` returns world `bounds` (text bounds synthesized when degenerate). |
| Entity type/property inspection (`RXClass`, overrides) | Exists — `record_schema` per collection/type. |

### 2.2 Selection and spatial query (Catalog §3.1–3.8)

| .NET API | OpenCAD status |
|---|---|
| `Editor.SelectCrossingWindow` + `SelectionFilter` (TypedValue lists) | Exists — `select` (handles/type/layer) and `query` with `layer`, `type`, `handles`, world-XY `bounds`, `near` (kernel distance ranking), `contains_point`, exact `intersections`. TypedValue-style cross-object filters (`where` on any property path) exist for records; **Proposed P1** — the same `where` filter list on `query` entities. |
| Point-in-polyline (`MPolygon`, Catalog §3.6) | Exists — `contains_point` over closed planar curves. |
| Curve/curve intersections, nearest point, length/area | Exists — `query intersections`, `near` + `measure` (kernel). |
| Polyline segment extraction (Catalog §3.7) | Exists — `query detail:"full"` exposes vertices/properties; **Proposed P2** — arc-segment bulge normalization if clients need exact arc data in one call. |
| Hatch boundary extraction (Catalog §3.8) | **Proposed P1** — `query` on Hatch returns loop definitions under `properties`; document the shape. |
| Named selection sets with filters | **Proposed P2** — `selection_sets` create/list/reuse; today clients re-send the filter. |

### 2.3 Create / modify / erase (the main missing DatabaseServices surface)

| .NET API | OpenCAD status |
|---|---|
| `AppendEntity` (typed entity creation) | **Shipped** — `entities_create` — `entities_create` (batch, typed): <br>`{"op":"entities_create","entities":[{"type":"Line","start":[0,0,0],"end":[100,0,0],"layer":"Walls"},{"type":"LwPolyline","vertices":[[x,y],…],"closed":true},{"type":"Hatch","pattern":"ANSI31","scale":2,"boundary":[[x,y],…]},{"type":"Text","value":"…","position":[x,y,z],"height":2.5}, …]}` → `{"handles":[…]}`. Covers Catalog §6 (template frames), §3.9 (hatch remain), §1.4 (first draw on a layer). Geometry validation, undo snapshot and layer-existence checks happen server-side in one atomic step. |
| `entity.Erase()` | **Shipped** — `entities_delete` — `entities_delete` with `handles` (block-ownership rules enforced like ERASE). |
| `entity.TransformBy / Matrix3d` (move/copy/rotate/scale/mirror/array) | **Shipped** — `entities_transform` — `entities_transform` with `action` ∈ `move\|copy\|rotate\|scale\|mirror\|array`, `handles`, and action parameters (displacement, center+angle, base+factor, axis, rows/columns/spacing). Copy returns the new handles. Today these are reachable only through interactive command syntax via `run`. |
| `entity.LayerId = …` before append (create-if-missing, Catalog §1.4) | Partial — `layers` + `records` can create layer records; **Proposed P0 nicety** — `entities_create` accepts `"create_layers":true`. |
| `BlockTableRecord` + `AppendEntity` into a block (Catalog §2.3) | **Shipped** — `block_define` — `block_define` `{"name":"…","base":[x,y,z],"handles":[…]}` creates a definition from existing entities and optionally places one `Insert`; result returns definition + insert handles. |
| `DeepCloneObjects` / `CopyObjects` between databases | Partial — `wblock` (shipped) clones to a *file*; **Proposed P1** — `entities_copy_to` targeting another open `document_id`, and `wblock` gaining `"template":"…dwt"` (Catalog §2.1) so the new database inherits the template's tables/styles. |
| New database from template (`DocumentManager.Add(dwt)`, Catalog §2.1–2.2) | **Proposed P1** — `new` gaining `"template":"path.dwt"` and `save` gaining `"format":"dwt"`. |
| `XData` / `RegAppTable` (Catalog §1.5–1.6) | **Shipped** — `xdata_set`/`xdata_get`; entity records also expose `extended_data` — `xdata_set` `{"handles":[…],"app":"SPM","data":{"1000":"tag","1070":42}}` with implicit RegApp registration, and `xdata_get`/`xdata_clear`. This is SPM's marking mechanism and several workflows depend on it. |
| `Group` dictionary | **Proposed P2** — `group_create`/`group_add`; rare outside AutoCAD-centric tooling. |
| `Layout`/`LayoutManager` create + `PlotSettingsValidator` (Catalog §4 preparation) | **Proposed P1** — `layout_create` `{"name":…,"paper":…,"printer_config":…}` and `page_setup_set` against an existing layout (paper, area, scale, style table) — the API counterpart of what the `plot` op consumes. |

### 2.4 Transactions and undo (parity achieved differently)

`TransactionManager`/`OpenClose` are replaced by the protocol's guarantees:
every op is one atomic, undoable commit with compare-and-set revisions and
idempotent replay — a client "transaction" is a `batch` (MCP) or a sequence of
ops against a held `revision`. No work proposed.

## 3. Editor (AcCoreMgd `EditorInput`)

| .NET API | OpenCAD status |
|---|---|
| `CommandMethod` (defining commands) | Plugin API (`ocs_plugin_api`) — out of automation scope, by design. |
| Prompt machinery (`GetPoint/GetEntity/GetString/GetKeyword`) | Exists — `start` + `input` with `state.command.accepts/options/input_example` guidance. |
| `Editor.WriteMessage` / history | Exists — `history` op. |
| `Editor.CurrentUserCoordSystem`, UCS handling | Exists — commands (`UCS`) + WCS/UCS/relative `input` spaces. |
| Focus/highlight an object (`FocusObjectByRowHandle`, Catalog §1.3) | **Shipped (GUI sessions)** — `view_focus` — `view_focus` `{"handles":[…],"highlight":true}`: zoom-to-bounds + transient highlight, ignored headless. |

## 4. Events (reactors)

| .NET API | OpenCAD status |
|---|---|
| Database/document/command events | Exists — cursor-paged `events` stream (128-event ring, `resync` flag) covering command lifecycle, document and selection changes. **Proposed P2** — type filters on the stream; polling model is deliberate (no push server). |

## 5. User interface and customization (AcMgd UI, AcCui)

| .NET API | OpenCAD status |
|---|---|
| Palette sets, modeless dialogs, ribbons | **N/A by design** — automation clients bring their own UI; OCS exposes `capture` and 20 `action` UI toggles instead. |
| CUI customization API | **P2** — OCS supports CUI files natively; no automation ops planned unless a client needs to author them. |

## 6. Layouts, plotting, publishing (AcCoreMgd `PlottingServices`/`PublishServices`)

| .NET API | OpenCAD status |
|---|---|
| `PlotEngine` + `PlotInfo` + `PlotSettingsValidator`, CTB/STB | Exists — `plot` (PDF; model/layouts/all; extents/display/limits/window/layout; paper catalog; fit/scale; plot styles). |
| `PlotConfig` (pc3 device management) | N/A — PDF-only output device; paper catalog replaces pc3 media queries. |
| Multi-sheet publish (one file per layout, DWF) | Partial — `layout:"all"` yields one multi-page PDF; **Proposed P1** — `"per_page":true` splitting layouts into one PDF each (client-side today), and **P2** DWF/SVG/PNG output formats. |
| Plot to raster (PNG preview) | Exists — `capture` (viewport/window screenshot) covers preview needs; exact-paper PNG lands with the P2 format work. |

## 7. Geometry, colors, measurement

| .NET API | OpenCAD status |
|---|---|
| `Autodesk.AutoCAD.Geometry` (Ge: Point/Vector/Matrix/Curve/BRep) | Exists in-process for plugins; automation exposes kernel results instead of raw types: `query near/contains_point/intersections`, `measure` (length, area, bounds). |
| Region/MPolygon booleans (Catalog §3.5) | **Proposed P1** — `region_boolean` (`union/intersection/difference`) and closed-region containment over existing curves, implemented in the geometry kernel. |
| `EntityColor`, color books | Exists — records expose `color` (ACI + true color), `set_properties` validates via `record_schema`. |

## 8. File-level operations

| .NET API | OpenCAD status |
|---|---|
| `Database.WblockCloneObjects` to file / `Database.SaveAs` | Exists — `wblock`, `save` (dwg/dxf). |
| Raster image attach (`RasterImageDef`/`RasterImage`, Catalog §5) | Exists — `embed_image` (embedded OLE2FRAME default, `"linked":true` path-linked). **Proposed P1** — image placement inside block definitions (Catalog §5.2/5.3) once `block_define` ships. |
| Transmittal (eTransmit) | **P2** — a packaging op only if a client needs it; clients can zip `wblock` outputs themselves. |
| Format conversion | Exists — `--export IN OUT` (dwg/dxf) headless. |

## 9. Consolidated work queue (priority order)

| # | Operation | Parity target | Unblocks | Priority |
|---|---|---|---|---|
| 1 | `entities_create` (typed batch) | `AppendEntity` | Catalog §6, §3.9, §1.4; any drawing client | **Shipped** |
| 2 | `entities_delete` | `Erase` | cleanup flows | **Shipped** |
| 3 | `entities_transform` (move/copy/rotate/scale/mirror/array) | `TransformBy` | layout/nesting clients | **Shipped** |
| 4 | `xdata_set` / `xdata_get` (+ RegApp) | `XData`/`RegAppTable` | Catalog §1.5–1.6 marking, SPMDOCUNIQUE | **Shipped** |
| 5 | `block_define` (+ optional Insert) | `BlockTableRecord` | Catalog §2.3 barcode block | **Shipped** |
| 6 | `view_focus` | `SetCurrentView`+highlight | Catalog §1.3 (GUI sessions) | **Shipped** |
| 7 | `where` filters on `query` | `SelectionFilter` TypedValues | precise cross-property selection | **P1** |
| 8 | `close` | `Document.Close` | MDI housekeeping | **P1** |
| 9 | `sysvar` get/set | System variables | AutoCAD-habit scripts | **P1** |
| 10 | `layout_create` / `page_setup_set` | `LayoutManager`/`PlotSettingsValidator` | sheet provisioning before `plot` | **P1** |
| 11 | `wblock` template + `new` from template + `save` `.dwt` | `DocumentManager.Add(dwt)` | Catalog §2.1–2.2 | **P1** |
| 12 | `entities_copy_to` (cross-document) | `CopyObjects` | multi-document assembly | **P1** |
| 13 | `region_boolean` + hatch loop readout | `Region`/`MPolygon`, hatch | Catalog §3.5/3.8 nesting remain | **P1** |
| 14 | `plot` per-page PDFs; SVG/PNG formats | Publish | sheet-by-sheet delivery | **P1** |
| 15 | Selection sets, groups, event filters, DWF, eTransmit | misc | niche | **P2** |

Protocol conventions for every new op: envelope `protocol:1`, caller
`request_id` (replay = cached result), `document_id` addressing, CAS via
`revision`/`geometry_revision`, one undo step per op, `capabilities` lists
each shipped op so clients can feature-detect.
