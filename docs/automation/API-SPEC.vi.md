# OpenCADStudio Automation API — Đặc tả kỹ thuật (bản tiếng Việt)

> **Bản dịch tiếng Việt** của [`API-SPEC.md`](API-SPEC.md) — bản tiếng Anh là
> bản chuẩn, bản này có thể tụt lại sau khi bản chuẩn cập nhật. Tên op, tên
> trường, mã lỗi, đường dẫn và code mẫu **giữ nguyên tiếng Anh** vì đó là giá
> trị gửi trên dây truyền (wire) — chỉ phần diễn giải được dịch.

Phiên bản đặc tả **1.3** · Giao thức API **1** · áp dụng cho các bản fork
`2026.41` trở lên (`2026.40` cho mọi thứ trừ §3.5 và §6.12).
Bản build `2026.46` sửa tham số truy vấn số của REST (`limit`/`offset` trước
đây bị bỏ ngầm, khiến mọi phản hồi `GET /entities` / `GET /records` bị ghim
vào 1000 dòng đầu); mục §6.4 "Pagination" mô tả hành vi đã sửa.

Spec **1.3** biến §6 thành tham chiếu đầy đủ theo từng thao tác: mọi mutation
và mọi read đều có bảng trường riêng và ví dụ request/response hoàn chỉnh.
Bổ sung các op cập nhật sau spec 1.2 — `switch_document`, `grip_drag`,
`click`, `pointer`, `text_replace`, `save_verified`, `stop` — và ghi nhận
port mặc định của REST cùng cơ chế discovery qua file descriptor (§3.1).

Tài liệu này là tham chiếu đầy đủ, tự containment cho automation API của
OpenCADStudio. Mọi thứ một client cần — quy ước, transport, danh mục thao tác,
luật kiểm tra hợp lệ, mã lỗi và ví dụ mẫu — đều nằm ở đây. API trung tính với
transport: một dispatcher duy nhất phục vụ mọi client, nên một request có ngữ
nghĩa giống hệt nhau dù đi qua port HTTP local, pipe stdio, socket TCP hay
phiên công cụ AI (MCP).

Thiết lập phiên MCP và hướng dẫn REST thực hành nằm trong
[`docs/automation/README.md`](README.md); file này liên kết tới đó thay vì
lặp lại.

---

## 1. Tổng quan

| Khái niệm | Ý nghĩa |
|---|---|
| **Session** | Một tiến trình editor đang chạy. Sở hữu session id, con trỏ sự kiện và 1…N bản vẽ đang mở. |
| **Document** | Một bản vẽ (một tab). Định danh bằng `document_id` ổn định. Tại mỗi thời điểm đúng một document ở trạng thái **active**. |
| **Handle** | Định danh của mọi đối tượng cơ sở dữ liệu — một chuỗi thập lục phân (ví dụ `"2A"`). Handle ổn định qua save/open và là khóa cho mọi thao tác đọc, sửa, xóa, nhân bản. |
| **Operation** | Một bước nguyên tử, undo được. Được kiểm tra hợp lệ toàn bộ trước khi có gì commit; request bị từ chối thì không thay đổi gì. |
| **Envelope** | Bộ khung bọc mutation: `"protocol":1`, một `request_id` do caller sinh ra, và `document_id` đang được sửa. |
| **Pick** | Thao tác có con người tham gia (`user_select`, `getpoint`, §6.12): op ở trạng thái chờ trong khi *con người* trả lời tại màn hình. |

Luật thiết kế, theo thứ tự ưu tiên:

1. **Idempotency** — một mutation phát lại với *cùng* `request_id` trả về kết
   quả đã cache thay vì thực thi hai lần. Sinh một `request_id` mới cho mỗi
   thao tác logic (≤ 128 byte) và chỉ dùng lại khi retry sau timeout.
2. **Trạng thái lạc quan (optimistic state)** — đọc `state` trước, truyền lại
   `document_id` (và `revision`/`geometry_revision` nếu đang giữ) khi gọi
   mutation. Một lời từ chối có mã trong nhóm retryable (§5.2) nghĩa là
   "snapshot của bạn đã cũ": refresh state và retry một lần với `request_id`
   *mới*.
3. **Nguyên tử (atomicity)** — mọi thao tác hoặc-thành-công-hoặc-không và tạo
   đúng một bước undo. Giao dịch nhiều bước là op `batch` của client, hoặc
   một chuỗi op giữ nguyên `revision`.
4. **Hình học nằm trong engine** — đo đạc, kiểm tra chứa, giao điểm và xếp
   hạng do geometry kernel tích hợp sẵn tính; client không tự cài lại.

---

## 2. Quy ước

| Quy ước | Giá trị |
|---|---|
| Góc | Tính bằng **độ** trên dây truyền, dương ngược chiều kim đồng hồ (`rotation_deg`, `angle_deg`, `start_angle_deg`, `end_angle_deg`) |
| Điểm | `[x, y]` (z = 0) hoặc `[x, y, z]`, tọa độ world (WCS). Nhập tương tác nhận thêm `"space":"ucs"` / `"relative"`. |
| Khoảng cách, kích thước | đơn vị bản vẽ (chỉ millimet khi trường là paper-space rõ ràng, ví dụ `paper_width`) |
| Màu | chỉ số nguyên `1–255` (`1` đỏ … `7` trắng), `256` theo layer (by-layer), `0` theo block (by-block); true color được nhận nơi có ghi chú |
| Handle | chuỗi thập lục phân, không phân biệt hoa thường, không cần tiền tố `0x` |
| Tên | tên layer / block / style duy nhất không phân biệt hoa thường; tên có tiền tố `*` là dành riêng |
| Boolean | JSON `true`/`false` |
| Template | file `*.dwt` được ghi và đọc như byte DWG — cùng container, đuôi template |
| Khổ giấy | tên từ catalog, ví dụ `"ISO_A4_(210.00_x_297.00_MM)"` (tên canonical được trả lại trong kết quả) |
| Tỷ lệ | `"fit"` hoặc tỷ lệ `"paper:drawing"` kiểu `"1:100"` (cả hai vế > 0) |

---

## 3. Transport

Mọi transport nói chuyện với cùng một dispatcher; mỗi client chọn một. REST
và MCP quảng bá toàn bộ bề mặt API; dò tính năng bằng `capabilities` (§4).

### 3.1 HTTP REST — headless (`--http <port>`)

`OpenCADStudio --http 8090` chạy một session riêng, không cửa sổ, sau một bề
mặt REST định hướng tài nguyên tại `http://127.0.0.1:<port>/api/v1` (chỉ bind
loopback, body JSON, một request mỗi kết nối, không header CORS). Mô tả
OpenAPI 3 đọc bằng máy của toàn bộ bề mặt được phục vụ tại
`GET /api/v1/openapi`.

Mỗi request mang `Authorization: Bearer <token>`: giá trị của `OCS_API_TOKEN`
nếu launcher đặt, nếu không là một token ngẫu nhiên in ra stderr khi khởi
động. Request thiếu token bị trả **401** `{"code":"unauthorized"}`. Loopback
dùng chung bởi mọi tiến trình và người dùng trên máy, nên riêng port không
phải là thông tin xác thực.

**Port mặc định và discovery** — `--http` là tùy chọn: GUI mở không cờ nào
cũng phục vụ bề mặt này trên `127.0.0.1:7931`. Kênh tự công bố bằng cách ghi
một descriptor vào thư mục config của hệ điều hành,
`%APPDATA%\OpenCADStudio\automation\http.json` trên Windows
(`…/OpenCADStudio/automation/http.json` tại thư mục config tương đương trên
Linux/macOS):

```json
{"protocol":1,"pid":12345,"port":7931,"token":"…","default":true}
```

| Trường | Ý nghĩa |
|---|---|
| `port` | Port đang sống — trùng giá trị `--http` khi cờ được đưa |
| `token` | Bearer token gửi kèm mọi request (§3.2 dùng chung) |
| `pid` | Id tiến trình sở hữu — client có thể kiểm tra kênh còn sống trước khi gọi |
| `default` | `true` khi kênh này là port mặc định không-cờ (7931), `false` khi do `--http` chỉ định |

Nhờ đó người dùng chỉ cần mở bản vẽ từ shortcut desktop và một script tự tìm
thấy API đang chạy mà không cần biến môi trường đặt sẵn: đọc `http.json`, gửi
`Authorization: Bearer <token>` tới `127.0.0.1:<port>/api/v1`. `--http PORT`
rõ ràng đè lên mặc định; khi port mặc định đã bị chiếm, instance sau in một
gợi ý ra stderr và không phục vụ gì (`--http` trên port trống luôn hoạt động).
Descriptor được ghi lại mỗi khi một kênh bắt đầu listen, nên editor mới nhất
chiếm file.

| Mã HTTP | Ý nghĩa |
|---|---|
| `200` | Thành công (đọc hoặc cập nhật) |
| `201` | Một tài nguyên đã được tạo (document, entity, block, group, selection set, layout) |
| `400` | Lỗi kiểm tra hợp lệ — body giữ mã `code` mang tính biểu tượng và thông điệp |
| `401` | Thiếu hoặc sai bearer token (`code:"unauthorized"`) |
| `404` | Route lạ hoặc handle lạ (`code:"entity_absent"`) |
| `409` | Xung đột trạng thái (`stale_state`, `document_not_active`) hoặc cần GUI (`gui_required`) |
| `503` | Dispatcher bận |

Tầng REST tự sinh `request_id` và cache `document_id` đang active phía server,
retry một lần với các lời từ chối retryable — client HTTP thuần không cần sổ
sách envelope. Client nâng cao có thể POST tên op bất kỳ tới `/api/v1/{op}`
với body tường minh — kể cả `operation` (poll một op đang chờ) và `cancel`.

Mỗi kết nối được phục vụ bởi một thread riêng: một op chậm (`plot`, mở bản vẽ
lớn) hay một client kết nối rồi đứng im chỉ giữ kết nối của chính nó, không
bao giờ cản các caller khác. Bản thân các op vẫn thực thi lần lượt trên
session riêng tư (cùng một pipeline một-dispatcher như mọi transport), nên
độ đồng thời mua được độ phản hồi, không mua mutation song song. Một kết nối
không gửi gì trong **15 giây** bị ngắt.

### 3.2 HTTP REST — GUI host (`--http <port>` **kèm file bản vẽ**)

`OpenCADStudio.exe "<file>" --http 8090` khởi động **editor bình thường** và
host cùng bề mặt REST ngay trên tiến trình này, nhắm vào bản vẽ người ngồi
trước màn hình đang làm: cùng route, cùng ngữ nghĩa, cộng thêm các pick
human-in-the-loop của §6.12 (cần cửa sổ mà tiến trình này sở hữu). Những điểm
khác biệt đáng biết:

- `GET /state` nạp trước cache `document_id` phía server; các mutation sau đó
  nhắm thẳng vào bản vẽ đang sống mà caller không phải lặp lại id. Cache cũ
  (người dùng đổi document) chỉ tốn một lần refresh state và một lần retry,
  không phải lỗi.
- Không cần `session_id` (kênh không có bắt tay descriptor; giá trị đoán bừa
  bị lược bỏ).
- Một thread mỗi kết nối: một pick đang chờ không bao giờ chặn các call khác.
  Một pick có thể chờ tới **30 phút**; op khác trả lời trong vòng **300 giây**
  hoặc bridge trả `response_timeout`.
- Pick đang chờ tự canh connection của nó: ngay khi client gác máy (timeout
  của chính nó, crash, probe bị kill), pick chờ bị rút lại bằng `cancel` và
  kênh tự do trở lại — pick kế tiếp bắt đầu sạch thay vì chết mãi với
  `interactive_pending` cho tới khi người dùng tình cờ bấm Esc. Cơ chế rút lại
  này cũng chạy nếu caller chờ hết trọn 30 phút.
- Mỗi pick ghi vòng đời ra stderr dưới dạng dòng `[pick] <op> <request_id>`
  (bắt đầu, hủy-vì-mất-kết-nối, hoàn tất), một dòng mỗi trạng thái, để log
  phía client đối chiếu được từng dòng.
- POST tới path lạ được coi là lần gọi op và được chuyển tiếp (mọi op của bề
  mặt đều chạy — kể cả `operation` để poll một pick đang chờ và `cancel` để
  rút nó); path thật sự không có route trả `404 {"code":"unknown_route"}`.
- Cần cùng bearer token như §3.1 trên mọi request.

### 3.3 stdio JSONL (`--serve`)

Một giá trị JSON mỗi dòng qua stdin/stdout. Dòng stdout **đầu tiên** là lời
chào sẵn sàng: `{"ok":true,"ready":true,"version":"…","session_id":"…"}`.
Mỗi dòng sau là một cặp request/response (read kiểu legacy không cần
envelope; mutation dùng envelope của §1).

### 3.4 TCP (`--serve --port N`)

Cùng giao thức JSONL như §3.3 qua `127.0.0.1:<N>`, một client mỗi lúc. Mỗi
dòng request mang `"token":"<token>"` (token của §3.1); dòng thiếu token bị
trả `{"ok":false,"code":"unauthorized"}` và kết nối bị đóng.

### 3.5 Phiên công cụ AI (`--mcp`)

Cung cấp bốn tool — `ocs_sessions`, `ocs_read`, `ocs_execute`, `ocs_capture`
— với tham số được JSON-Schema kiểm tra (một mình `ocs_execute` nhận cả 44
mutation op, §6). `ocs_read` phục vụ thêm schema tạo entity sinh theo từng
loại (`entity_schema`) và các hướng dẫn theo yêu cầu. Dò session và quảng bá
capability theo cùng ngữ nghĩa protocol-1. Hướng dẫn cài đặt: xem mục MCP của
[`README.md`](README.md).

---

## 4. Session và trạng thái

`{"protocol":1,"op":"state"}` (hoặc `GET /api/v1/state`) trả về mọi thứ client
cần để hành động:

| Trường | Ý nghĩa |
|---|---|
| `session_id` | Định danh của session đang chạy; bắt buộc với transport MCP, nơi khác nhận-và-bỏ-qua (chỉ giá trị *sai* mới bị từ chối) |
| `document_id` | Document đang active |
| `revision`, `geometry_revision` | Token optimistic-concurrency cho dữ liệu và hình học |
| `hand_seed` | Handle seed của bản vẽ, hex — bộ đếm đối tượng theo file, tăng đơn điệu |
| `documents[]` | Mọi document đang mở: `id`, `title`, `path`, `dirty`, `revision` |
| `selection[]` | Các handle đang được chọn, theo thứ tự pick |
| `command` | Trạng thái của một lệnh tương tác đang chạy (nếu có): `accepts[]`, `options[]`, `input_example` |
| `operation` | `request_id` của thao tác đang chờ, nếu có |
| `capabilities` | Danh sách tính năng (cũng có dưới dạng read `capabilities` riêng) |

`hello` là alias của `state`. `file_identity` (§6.1) cho thêm mỗi bản vẽ một
GUID ổn định.

---

## 5. Mô hình lỗi

### 5.1 Envelope phản hồi

Mọi phản hồi hoàn tất đều mang:

```json
{"ok":true,"status":"completed","request_id":"…","result":{…},"changes":[…],"state":{…}}
```

`status` là một trong `completed`, `accepted`, `running` (việc bất đồng bộ —
poll bằng `{"protocol":1,"op":"operation","request_id":…}`), `waiting_input`
(một lệnh tương tác muốn thêm dữ liệu vào), `cancelled`, `failed`. `changes[]`
liệt kê các handle bị ảnh hưởng (giới hạn 1000) để client refresh rẻ.

### 5.2 Mã lỗi

Lỗi có dạng `{"ok":false,"code":"…","error":"…"}`. `code` mang tính biểu tượng
và ổn định; hãy rẽ nhánh theo nó, đừng theo nội dung thông điệp.

| Mã | Ý nghĩa | Retry được? |
|---|---|---|
| `stale_state`, `session_changed` | Snapshot của caller đã cũ | có — refresh `state`, retry một lần với `request_id` mới |
| `document_required`, `document_not_active`, `document_closed` | Lập địa chỉ envelope sai | có (refresh state) |
| `document_dirty` | Đóng sẽ mất thay đổi chưa lưu | không — gửi lại với `"discard":true` để xác nhận |
| `entity_absent` | Một handle trong danh sách không tồn tại (thông điệp nêu tên) | refresh và đọc lại |
| `command_busy` | Một lệnh tương tác đang chạy | hoàn tất hoặc `cancel` trước |
| `gui_required` | Thao tác cần cửa sổ editor (`view_focus` và các pick của §6.12 trên transport headless) | chỉ session headless |
| `busy` | Việc bất đồng bộ vẫn đang chạy | poll `operation` |
| `interactive_pending` | Một pick của người dùng khác (§6.12) đang chờ | chờ nó, hoặc rút bằng `cancel` |
| `user_input_busy` | Người dùng đang giữa lệnh hoặc trong hộp thoại; phím Enter/Esc thuộc về họ | chờ rồi retry |
| `invalid_detail` | `detail` của `user_select` ngoài `summary`/`geometry`/`full` | không |
| `selection_changed` | Tiền điều kiện `selection[]` không còn khớp | đọc lại và xác nhận |
| `name_required`, `entities_required`, `selection_required`, `app_required`, `path_required`, `handles_required`, `block_required`, `missing_find` | Thiếu trường bắt buộc | không |
| `absolute_path_required` | `save_verified` nhận đường dẫn tương đối | gửi lại với đường dẫn tuyệt đối |
| `unknown_entity_type`, `invalid_radius`, `invalid_vertices`, `invalid_point`, `invalid_text`, `invalid_scale`, `invalid_paper`, `invalid_handle`, `invalid_xdata`, `invalid_sysvar_value`, `invalid_sweep`, `unknown_entity_field` | Lỗi kiểm tra hợp lệ — không có gì commit | không |
| `unknown_sysvar` | Biến ngoài registry (§6.7) | không |
| `disabled` | Automation mutation đã bị dừng (op `stop`, §6.10) — bật lại trong GUI | không (thủ công) |
| `no_document` | Op lệnh (`run`/`start`/`input`) khi chưa mở bản vẽ nào | tạo hoặc mở một bản vẽ |
| `block_editor_open` | `entities_create` khi block editor đang mở một block | hoàn tất chỉnh block trước |
| `document_not_found` | `document_id` không trỏ tới document nào đang mở | refresh `state` |
| `layout_exists`, `layout_missing`, `layer_failed`, `block_failed`, `block_missing`, `block_protected`, `block_referenced`, `block_in_edit`, `block_invalid_name`, `block_exists`, `selection_set_missing`, `template_missing`, `template_failed`, `save_failed` | Từ chối thuộc miền nghiệp vụ, tự giải thích được | không |
| `reopen_failed`, `version_mismatch`, `semantic_mismatch`, `invalid_dxf_handle_graph` | Kiểm tra roundtrip của `save_verified` (§6.1); file được giữ lại để chẩn đoán | sửa dữ liệu nguồn |

Route lạ trả `code:"unknown_route"` (HTTP 404). Request chờ bị timeout trả
`code:"response_timeout"`.

---

## 6. Tham chiếu thao tác

Bề mặt API là một danh mục duy nhất được phục vụ giống hệt bởi mọi transport:
**44 mutation** op (op có envelope, §1) và các **read** của §6.4. Cả 44 đều
được liệt kê trong quảng bá `capabilities` và trong schema MCP; REST
passthrough nhận bất kỳ op nào tại `POST /api/v1/{op}`.

Mỗi mục dưới đây có cùng cấu trúc để client vừa lướt vừa đọc sâu được:

1. **Mục đích** — một dòng.
2. **Trường request** — tên, kiểu, bắt buộc/tùy chọn, ý nghĩa. Kiểu theo §2
   (`Point` = `[x,y]` hoặc `[x,y,z]`, `handles` = mảng chuỗi hex, …).
3. **Kết quả** — những gì `result` mang trong envelope hoàn tất của §5.1
   (`request_id`, `changes[]`, `state` được lược thành `…` trong ví dụ).
4. **Ví dụ** — một request gửi được ngay và một phản hồi điển hình.

Op có nhãn **route** có đường REST riêng (bảng đầy đủ §6.11); op còn lại đi
qua passthrough với dạng tổng quát:

```sh
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST \
  "http://127.0.0.1:7931/api/v1/$OP" -H "Content-Type: application/json" \
  -d "$BODY"
```

### 6.1 Document, session, vòng đời

#### `new` — tạo document mới chưa có tên

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `template` | string | không | Đường dẫn template `*.dwt`/`*.dwg`/`*.dxf` — document mới thừa hưởng bảng, style và entity của template, và không giữ đường dẫn nguồn |

Kết quả: `total` (số entity nhập từ template). REST: **route**
`POST /documents` — `{}` (hoặc body rỗng) tạo document trống,
`{"template":…}` tạo theo template, `{"path":…}` thì mở file (op kế tiếp).

```json
{"op":"new","template":"C:/tpl/a3.dwt"}
→ {"ok":true,"status":"completed","result":{"total":137},"changes":[],"state":{…}}
```

#### `open` — mở bản vẽ có sẵn làm document active

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `path` | string | **có** | Đường dẫn tuyệt đối tới file `.dwg`/`.dxf`; mở trong tab riêng và thành active |

Kết quả: tóm tắt entity (tổng và đếm theo loại). REST: **route**
`POST /documents` với `{"path":…}`.

```json
{"op":"open","path":"C:/drawings/plan.dwg"}
→ {"ok":true,"status":"completed","result":{"total":4210,"by_type":{"Line":3900,…}}}
```

#### `activate` · `switch_document` — đổi tab active

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `document_id` | integer | **có** | Tab đang mở cần chuyển thành active |

`switch_document` là alias của `activate` — hợp đồng giống hệt, giữ lại cho
client thích tên tường minh.

```json
{"op":"activate","document_id":2}
```

#### `close` — đóng document active

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `discard` | boolean | không | `true` vứt bỏ thay đổi chưa lưu; thiếu nó, document có thay đổi bị từ chối với `document_dirty` |

Đóng tab cuối cùng để lại một bản vẽ trống mới. Kết quả: `closed`.
REST: **route** `DELETE /documents/{id}?discard=true`.

```json
{"op":"close","discard":true}
→ {"ok":true,"status":"completed","result":{"closed":1},…}
```

#### `save` — ghi document active ra đĩa

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `path` | string | khi chưa có tên | Đích ghi; `*.dwg`/`*.dxf` theo đuôi file, `*.dwt` ghi template (byte DWG) |
| `target_format` | `"dwg"` \| `"dxf"` | không | Định dạng đầu ra tường minh (phải khớp đuôi file) |
| `target_version` | tên phiên bản | không | `R14`, `2000`…`2018`, hoặc `AC1014`…`AC1032` |
| `allow_lossy` | boolean | không | Xác nhận các record mà phiên bản đích không biểu diễn được (chúng bị bỏ và được báo cáo) |

Kết quả: `saved`. REST: **route** `POST /save` với `{"path":…}` tùy chọn.

```json
{"op":"save","path":"C:/out/plan-2018.dwg","target_version":"AC1032"}
→ {"ok":true,"status":"completed","result":{"saved":"C:/out/plan-2018.dwg"},…}
```

#### `save_verified` — lưu, mở lại và chứng minh roundtrip

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `path` | string | **có** | Đích `.dwg`/`.dxf` *tuyệt đối* (đường tương đối bị trả `absolute_path_required`) |
| `target_format` | `"dwg"` \| `"dxf"` | không | Định dạng đầu ra tường minh khớp đuôi file |
| `target_version` | tên phiên bản | không | Phiên bản CAD tường minh (cùng giá trị với `save`) |
| `allow_lossy` | boolean | không | Xác nhận bỏ record |
| `overwrite` | boolean | không | Ghi đè file có sẵn thay vì từ chối |

Lựa ghi an toàn khi bàn giao: lưu, mở lại file vừa ghi, tính lại manifest
ngữ nghĩa và so với bản nguồn, kiểm tra đồ thị handle DXF, rồi trả về SHA-256
của file đầu ra. Bất kỳ lệch nào đều từ chối với `version_mismatch` /
`semantic_mismatch` / `invalid_dxf_handle_graph` và giữ file lại để chẩn đoán.
Không chạy trong `batch`.

```json
{"op":"save_verified","path":"C:/out/plan-final.dwg"}
→ {"ok":true,"status":"completed","verified":true,"saved":"C:/out/plan-final.dwg",
   "sha256":"9f2c…","bytes":183442,"target_format":"dwg","target_version":"AC1032",
   "dropped_on_save":[],"manifest":{…},"audit":{…}}
```

#### `file_identity` — GUID ổn định của bản vẽ

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `renew` | boolean | không | Đúc định danh mới thay vì trả định danh đã lưu |

Kết quả: `identity` (GUID RFC 4122 v4) và `created` (true ở lần gọi đầu).
GUID được đúc ở lần gọi đầu, lưu bên trong bản vẽ, sống qua save/open và
nhân bản. REST: **route** `POST /file-identity`.

```json
{"op":"file_identity"}
→ {"ok":true,"status":"completed","result":{"identity":"5db78b90-ca0b-4a4f-ad68-8c860483a41f","created":true},…}
```

#### `undo` · `redo`

Không có trường request; mỗi op đúng một bước undo trên document active.
REST: **route** `POST /undo` · `POST /redo`.

```json
{"op":"undo"}
```

### 6.2 Tạo entity — `entities_create`

`{"entities":[{…},…]}` — một loạt định nghĩa có kiểu, được kiểm tra hợp lệ
toàn bộ trước khi có gì commit; một bước undo; layer thiếu được tự tạo và
báo trong `result.layers_created`. Mọi định nghĩa còn nhận thêm `layer` và
`color` (chỉ số); trường khác bị từ chối (`unknown_entity_field`). Lệnh MCP
`ocs_read` `entity_schema` (không `type` → danh sách loại, hoặc
`parameters.type` → trường và ví dụ) trả lại bảng này theo kiểu lập trình.

| Kiểu | Trường |
|---|---|
| `Line` | `start`, `end`, tùy chọn `thickness` |
| `Circle` | `center`, `radius` (≥ 0) |
| `Arc` | `center`, `radius` (> 0), `start_angle_deg`, `end_angle_deg` (phải quét một cung hữu hạn khác 0) |
| `Spline` | `control_points` (nhiều hơn `degree`) hoặc `fit_points` (≥ 2), tùy chọn `degree` (1–10, mặc định 3), `knots` (control points + degree + 1, không giảm), `weights` (một giá trị dương mỗi control point), `closed`, `periodic`, `rational`, `planar`, `linear` |
| `LwPolyline` (alias `Polyline`) | `vertices` (≥ 2), `closed`, tùy chọn `constant_width` |
| `Point` | `location` |
| `Text` | `value`, `position`, tùy chọn `height` (mặc định 2.5), `rotation_deg`, `style` |
| `MText` | `value`, `position`, tùy chọn `height`, `width`, `rotation_deg`, `style` |
| `Insert` | `block` (phải tồn tại), `position`, tùy chọn `scale` (số hoặc `[x,y,z]`, khác 0), `rotation_deg` |
| `Solid` | `corners` (3 hoặc 4 `[x,y,z]`) |
| `Hatch` | `boundary` (≥ 3 điểm, kín), `solid`, hoặc `pattern` (tên catalog, mặc định `ANSI31`), `pattern_scale`, `pattern_angle_deg` |

Kết quả: `{"handles":[…],"created":N,"layers_created":[…]}` → HTTP 201.

Ví dụ — một loạt hỗn hợp; `"FRAME"` chưa tồn tại và được tạo:

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

Mỗi loại một định nghĩa, kiểu REST (route là `POST /entities`):

```sh
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST \
  http://127.0.0.1:7931/api/v1/entities -H "Content-Type: application/json" \
  -d '{"entities":[{"type":"Line","start":[0,0],"end":[100,0],"layer":"FRAME"}]}'
```

### 6.3 Sửa entity

#### `entities_delete` — xóa theo handle

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `handles` | string[] | **có** | Các handle hex cần xóa; mọi handle phải tồn tại — nếu không, không có gì bị xóa và `entity_absent` nêu tên cái thiếu |

Kết quả: `erased` (số lượng). REST: **route** `DELETE /entities?handles=63,64`.

```json
{"op":"entities_delete","handles":["63","64"]}
→ {"ok":true,"status":"completed","result":{"erased":2},…}
```

#### `entities_transform` — di chuyển / copy / xoay / scale / mirror / array

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `handles` | string[] | **có** | Entity cần biến đổi |
| `action` | enum | không | `move` (mặc định), `copy`, `rotate`, `scale`, `mirror`, `array` |
| `vector` | Point | move/copy | Vectors dịch chuyển `[dx,dy]` hoặc `[dx,dy,dz]` |
| `center` | Point | rotate/scale | Điểm gốc của phép xoay / scale |
| `angle_deg` | number | rotate | Độ, dương ngược chiều kim đồng hồ |
| `factor` | number | **scale** | Hệ số scale khác 0 (thiếu nó thì `scale` từ chối) |
| `axis` | Point[2] | **mirror** | Trục đối xứng dạng `[[x1,y1],[x2,y2]]` (thiếu nó thì `mirror` từ chối) |
| `copy` | boolean | mirror/copy | Giữ bản gốc; `copy`/`mirror+copy` trả handle mới trong `result.created` |
| `rows`, `columns` | integer | array | Mỗi cái 1–100, mặc định 1 |
| `row_spacing`, `column_spacing` | number | array | Bước lưới theo đơn vị bản vẽ |

REST: **route** `POST /entities/transform`.

```json
{"op":"entities_transform","handles":["63"],"action":"rotate","center":[50,25],"angle_deg":90}
→ {"ok":true,"status":"completed","result":{"affected":1,"created":[]},…}

{"op":"entities_transform","handles":["63"],"action":"copy","vector":[0,-20]}
→ {"ok":true,"status":"completed","result":{"affected":1,"created":["68"]},…}
```

#### `entities_copy_to` — nhân bản sang document đang mở khác

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `handles` | string[] | **có** | Entity của document *đang active* cần copy |
| `target_document_id` | integer | **có** | Tab nhận các bản copy |
| `document_id` | integer | lỗi thời | Alias cũ của `target_document_id`; cần một trong hai |

Định nghĩa layer được tham chiếu đi theo. Kết quả: `created[]` (handle mới)
+ `count`. REST: **route** `POST /entities/copy-to`.

```json
{"op":"entities_copy_to","handles":["63","64"],"target_document_id":2}
→ {"ok":true,"status":"completed","result":{"document_id":2,"created":["1F","20"],"count":2},…}
```

#### `text_replace` — tìm & thay trong các entity văn bản

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `find` | string | đi với `replace` | Văn bản cần tìm — cần đúng một trong `find` / `pairs` |
| `replace` | string | không | Văn bản thay thế; mặc định `""` (xóa chỗ khớp) |
| `pairs` | `{find,replace}[]` | đi với `find` | Nhiều cặp find/replace, áp dụng theo thứ tự |
| `match_case` | boolean | không | Tìm phân biệt hoa thường (mặc định không) |
| `whole_word` | boolean | không | Chỉ khớp nguyên từ |
| `ignore_accents` | boolean | không | Bỏ qua dấu (mặc định true khi `match_case` false) |
| `dry_run` | boolean | không | Chỉ báo cáo cái sẽ đổi — không mutation, không mục undo |
| `replace_all` | boolean | không | Thay mọi lần xuất hiện trong entity khớp (mặc định true; false chỉ thay lần đầu) |
| `scope` | enum | không | `all` (mặc định), `active_space`, `model_space`, `blocks` |
| `layer` | string | không | Lọc theo tên layer |
| `handles` | string[] | không | Lọc theo handle |
| `type` | string | không | Lọc theo loại entity (TEXT, MTEXT, …) |
| `bounds` | number[4] | không | Lọc `[min_x,min_y,max_x,max_y]` |

Cả một lần chạy chỉ là một bước undo. Kết quả: `replaced` (tổng số lần xuất
hiện đã thay), `entities_changed` (số entity) và `changes[]` (mỗi entity bị
đụng tới một mục `{handle, type, before, after, replaced}` — sửa attribute
của INSERT thêm `attribute_tag`); `"dry_run":true` trả cùng báo cáo nhưng
không đổi gì.

```json
{"op":"text_replace","find":"OLD","replace":"NEW"}
→ {"ok":true,"status":"completed","result":{"dry_run":false,"replaced":4,
   "entities_changed":3,"changes":[{"handle":"6A","type":"Text","before":"OLD-1","after":"NEW-1","replaced":1},…]},…}

{"op":"text_replace","pairs":[{"find":"Ø","replace":"DIA "},{"find":"mm","replace":""}],
 "scope":"model_space","dry_run":true}
→ {"ok":true,"status":"completed","result":{"dry_run":true,"replaced":12,"entities_changed":5,"changes":[…]},…}
```

### 6.4 Truy vấn và bộ lọc không gian — `query`

Tham số (tất cả tùy chọn, kết hợp tự do — ngữ nghĩa AND):

| Tham số | Ý nghĩa |
|---|---|
| `type`, `layer` | Tên loại entity / tên layer |
| `handles` | Danh sách handle chính xác |
| `detail` | `summary` (định danh + common), `geometry` (+ trường hình dạng), `full` (+ toàn bộ thuộc tính đã serialize, gồm bounds) |
| `offset`, `limit` | Phân trang — `limit` mặc định 1000, tối đa 10000; xem [Pagination] dưới đây |
| `fields` | Chiếu trường (property paths) |
| `bounds` | Bộ lọc cửa sổ world `[x0,y0,x1,y1]` (các entity cắt hình hộp) — muốn kiểm tra "nằm trọn trong", so `bounds` của từng entity phía client |
| `near` | `[x,y(,z)]` — xếp hạng theo khoảng cách kernel tính |
| `contains_point` | `[x,y]` — các đường cong kín chứa điểm (test ray của kernel) |
| `intersections` | `[h1,h2]` — các giao điểm chính xác của hai đường cong phẳng (kernel) |
| `where` | Danh sách bộ lọc thuộc tính trên entity đã serialize (dưới đây) |

Bộ lọc `where` dùng JSON Pointer paths vào entity đã serialize
(`"/radius"`, `"/common/layer"`) với các toán tử `eq`, `ne`, `lt`, `lte`,
`gt`, `gte`, `contains`, `starts_with`, `ends_with`, `in`, `exists`,
`not_exists`. Bộ lọc được kiểm tra trước; pointer sai hoặc toán tử lạ làm
truy vấn fail trước khi một entity nào được xét. Trên REST, cùng bộ lọc đi
trên `GET /entities?where=<json-encoded>`.

Entity văn bản trả chuỗi thô trong `value` cộng một bản dựng không định dạng
trong `text`; bounds của văn bản bề rộng suy biến được nới bằng một ước lượng
đã tài liệu hóa (height × 0.8 × số ký tự) để bộ lọc vùng vẫn dùng được.

#### Pagination

Mọi read phân trang — `query` lẫn `records` — trả về `count` (tổng khớp),
`returned` (số dòng của trang này) và `next_offset`:

| Trường | Ý nghĩa |
|---|---|
| `count` | Tổng khớp cho bộ lọc, độc lập với phân trang |
| `returned` | Số dòng trong phản hồi này (≤ `limit`) |
| `next_offset` | Offset của trang kế, hoặc **`null` khi đã hết** |

Mặc định và chặn trên: `offset` bắt đầu 0; `limit` mặc định **1000** và bị
kẹp tại **10000**. Các dòng sắp theo handle tăng dần — handle vượt quá 1000
đầu tiên chỉ với tới **qua** phân trang, nên client không bao giờ đi theo
`next_offset` sẽ chỉ lặng lẽ thấy một phần đầu của bản vẽ.

Mẫu vòng lặp (độc lập transport):

```json
{"op":"query","type":"Line","offset":0,"limit":1000}   → next_offset:1000
{"op":"query","type":"Line","offset":1000,"limit":1000} → next_offset:2000
…                                                       → next_offset:null
```

Trên REST cùng chuyến đi là `GET /entities?type=Line&offset=…&limit=…`.
Tham số truy vấn số được parse thành số từ bản build `2026.46`; các bản trước
đó bỏ chúng (luôn là trang một) — phát hiện bằng cách kiểm tra `returned` có
khớp `limit` đã yêu cầu hay không. Một trang có `returned` nhỏ hơn `limit`
mà `next_offset` khác null không thể xảy ra.

REST: **route** `GET /entities` với mọi tham số trên query string
(`GET /entities/{handle}` là lối tắt lấy một entity chi tiết đầy đủ;
`/entities` không tham số là alias của `query`).

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

### 6.4.1 Danh mục read

Mọi read đều độc lập transport; cái có route REST riêng được đánh dấu
**route** (§6.11). Read không cần envelope.

#### `records` — record cơ sở dữ liệu phân trang · **route** `GET /records?collection=&type=&offset=&limit=`

| Tham số | Ý nghĩa |
|---|---|
| `collection` | Một tên collection; bỏ qua để lấy manifest các collection |
| `type` | Lọc record theo tên loại |
| `offset`, `limit` | Phân trang như `query` (mặc định 1000, tối đa 10000) |

Phản hồi: `revision`, `geometry_revision`, `collection`, `count`,
`returned`, `next_offset`, `records[]`. Thiếu `collection` thì phản hồi là
manifest các collection.

```json
{"op":"records","collection":"block_records","limit":2}
→ {"ok":true,"collection":"block_records","count":14,"returned":2,"next_offset":2,
   "records":[{"name":"MARK","handle":"7A",…},…]}
```

#### `record_schema` — registry các trường ghi được

Không `collection` → manifest `[{name, mutable, root_type, identity_paths,…}]`.
Có `collection` (và tùy chọn `type`) → schema sinh ra cho collection đó: mọi
trường với kiểu, giá trị enum, đơn vị, và luật ghi mà `set_properties` (§6.7)
thực thi. Dựa vào đó sinh form phía client.

```json
{"op":"record_schema","collection":"layers"}
→ {"ok":true,"name":"layers","mutable":true,"root_type":"Layer",
   "identity_paths":["/name"],"fields":[{"/name":…,"/color/index":…,"/is_locked":…},…]}
```

#### `layers` — bảng layer · **route** `GET /layers`

Không tham số (cộng phân trang `offset`/`limit` tiêu chuẩn). Phản hồi:
`current` (layer active), `count`, `next_offset`, và `layers[]` — mỗi cái
`{name, off, frozen, locked, color?, rgb?}` (chỉ số ACI và/hoặc `[r,g,b]`).

```json
{"op":"layers"}
→ {"ok":true,"current":"0","count":3,"next_offset":null,
   "layers":[{"name":"0","off":false,"frozen":false,"locked":false,"color":7},
             {"name":"FRAME","off":false,"frozen":false,"locked":false,"color":1}]}
```

#### `header` — các biến header của document · **route** `GET /header`

Không tham số. Trả `current_layer`, `current_text_style`, `insertion_units`,
`pdmode`, `pdsize`, `ltscale`, `annotation_scale_value`.

```json
{"op":"header"}
→ {"ok":true,"current_layer":"0","current_text_style":"Standard","insertion_units":4,
   "pdmode":35,"pdsize":0.0,"ltscale":1.0,"annotation_scale_value":1.0}
```

#### `properties` — nội dung panel Properties của selection hiện tại

Không tham số; đọc selection hiện tại. Phản hồi: `sections[]`, mỗi cái
`{title, properties[]}` — chính các dòng mà panel Properties hiển thị.

```json
{"op":"properties"}
→ {"ok":true,"sections":[{"title":"Geometry","properties":[{"name":"Radius","value":"10"},…]},…]}
```

#### `measure` — số đo kernel của các handle liệt kê

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `handles` | string[] | không | Handle hex; **bỏ qua để đo selection hiện tại** |

Phản hồi: `document_id`, `geometry_revision`, `measurements[]` — mỗi handle
một mục: `bounds` (`min`/`max`), `curve` (`bounded`, `closed`, `length`,
`area` cho đường cong phẳng kín) và với solid là các số đo `mesh`
(`vertices`, `triangles`, `surface_area`, `volume`, `centroid`,
`moment_of_inertia`, `principal_directions`, `principal_moments`,
`product_of_inertia`, `radii_of_gyration`).

```json
{"op":"measure","handles":["64"]}
→ {"ok":true,"document_id":1,"geometry_revision":9,"measurements":[
    {"handle":"64","type":"Circle","bounds":{"min":[40,15,0],"max":[60,35,0]},
     "curve":{"bounded":true,"closed":true,"length":62.83,"area":314.16}}]}
```

#### `snap` — điểm cursor sẽ bắt được object snap nào

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `point` | Point | **có** | Điểm world cần dò |
| `from` | Point | không | Điểm gốc cho snap vuông góc/tiếp tuyến |

Phản hồi: `snap` (tên hoặc null) và điểm `world` đã bắt.

```json
{"op":"snap","point":[51,26]}
→ {"ok":true,"snap":"Center","world":[50,25,0]}
```

#### `grips` — các grip của selection hiện tại

Không tham số. Phản hồi: `grips[]` — `{handle, grip_id, point, shape,
moves_entity, axis?}`. Các id này nuôi `grip_drag` (§6.13).

```json
{"op":"grips"}
→ {"ok":true,"grips":[{"handle":"63","grip_id":0,"point":[0,0,0],"shape":"Square","moves_entity":false}]}
```

#### `xdata_get` — đọc extended data · **route** `GET /entities/{h}/xdata[?app=]`

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `handles` | string[] | **có** | Entity cần đọc |
| `app` | string | không | Một tên application; bỏ qua để lấy của mọi application |

```json
{"op":"xdata_get","handles":["64"],"app":"SPEC"}
→ {"ok":true,"records":[{"handle":"64","app":"SPEC","data":[{"code":1000,"value":"PAGE-01"}]}]}
```

#### `get_selection` — selection hiện tại, định dạng như `query`

Không tham số (nửa đọc-lại của `user_select`, §6.12). Phản hồi: `entities[]`
với `handle`, `type`, `display_type`, `layer`, `bounds`, `text`/`value` cho
entity văn bản, `block`/`position` cho insert, và toàn bộ thuộc tính đã
serialize. REST: **route** `POST /get_selection`.

```json
{"op":"get_selection"}
→ {"ok":true,"count":1,"entities":[{"handle":"6F","type":"Block Reference",
    "display_type":"Block Reference","block":"ANCHOR_A","position":[0,0,0],…}]}
```

#### `text_search` — tìm văn bản không đổi gì

Cùng các trường lọc với `text_replace` (§6.3: `find` bắt buộc; `match_case`,
`whole_word`, `ignore_accents`, `scope`, `layer`, `handles`, `type`,
`bounds`) cộng `limit` (mặc định 500, tối đa 5000) và `document_id`. Phản
hồi: các entity khớp với giá trị văn bản và vị trí.

```json
{"op":"text_search","find":"PAGE-"}
→ {"ok":true,"count":3,"matches":[{"handle":"66","type":"Text","value":"PAGE-01","position":[10,50,0]},…]}
```

#### `text_audit` — báo cáo chất lượng văn bản của bản vẽ

Cùng bộ lọc với `text_search`; trả các mục có vấn đề (thiếu style, văn bản
placeholder, rủi ro encoding) kèm gợi ý sửa.

```json
{"op":"text_audit"}
→ {"ok":true,"suspects":[{"handle":"71","type":"MText","issue":"missing_style","suggestions":["Standard"]},…]}
```

#### `events` — vòng đệm sự kiện

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `cursor` | integer | không | Sequence đã thấy cuối cùng; phản hồi mang mọi thứ sau nó |

Phản hồi: `cursor` (vị trí mới), `resync` (true khi vòng đệm đã quay vòng và
client nên refresh `state`), `events[]` — vòng đời lệnh, thay đổi document và
selection, vòng đệm 128 mục, thiết kế theo kiểu polling.

```json
{"op":"events","cursor":41}
→ {"ok":true,"cursor":43,"resync":false,"events":[{"sequence":42,"kind":"document",…},{…}]}
```

#### `history` — vệt dòng lệnh · REST `POST /history`

Không tham số. Phản hồi: `entries[]` dạng `{kind, text}` — mọi lệnh người
dùng *và* mọi client đã chạy, cũ nhất trước.

```json
{"op":"history"}
→ {"ok":true,"entries":[{"kind":"Client","text":"entities_create"},{"kind":"Command","text":"LINE 0,0 10,10"}]}
```

#### `capture` — chụp PNG viewport hoặc cửa sổ

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `path` | string | **có** | Đích file PNG |
| `scope` | enum | không | `viewport` (mặc định) hoặc `window` |
| `max_dimension` | integer | không | Chặn cạnh dài nhất tính bằng pixel, 256–4096, mặc định 1600 |
| `highlight_handles` | string[] | không | Chọn các handle này trước khi chụp |
| `focus_handles` / `bounds` / `view` | string[] / number[4] / enum | không | Khung ảnh: zoom tới handle, tới cửa sổ world, hoặc `extents`/`selection` |

Chỉ session GUI (headless trả `gui_required`). Phản hồi mang `path`, `scope`,
`width`, `height`, trạng thái camera và — để neo ảnh cho client thị giác —
`pixel_to_world_matrix`, `world_to_pixel_matrix` và `viewport_world_bounds`.

```json
{"op":"capture","path":"C:/out/shot.png","view":"extents","max_dimension":2048}
→ {"ok":true,"status":"completed","result":{"path":"C:/out/shot.png","scope":"window",
   "width":2048,"height":1158,"pixel_to_world_matrix":[…],…},…}
```

#### `audit` — báo cáo cấu trúc trước khi lưu

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `path` | string | không | Kiểm tra theo đích lưu này thay vì file hiện tại |
| `target_format`, `target_version` | enum | không | Đích bàn giao (cùng giá trị với `save`) |

Trả các kết luận cấu trúc (`save_verified` chạy cùng audit như một phần của
roundtrip): layer chưa khai báo, tham treo, record lossy.

```json
{"op":"audit","target_format":"dwg","target_version":"R14"}
→ {"ok":true,"audit":{"lossy_records":[],"undeclared_layers":[],"dangling_references":[]}}
```

MCP `ocs_read` cung cấp thêm `entity_schema` (§6.2), `validate` (preflight
một request với schema) và `guide` (hướng dẫn theo yêu cầu: `workflow`,
`architecture`, `capture`, `batch-bindings`).

### 6.5 Block

#### `block_define` — bọc entity thành block có tên

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `name` | string | **có** | Tên block (duy nhất, tiền tố `*` dành riêng) |
| `handles` | string[] | **có** | Các entity tạo nên định nghĩa — chúng chuyển vào trong (được làm phẳng tại `base` = gốc block) |
| `base` | Point | **có** | Điểm gốc block theo tọa độ world |
| `insert_at` | Point | không | Vị trí đặt INSERT thay thế (mặc định là `base`, nên bản vẽ nhìn như chưa đổi) |
| `replace` | boolean | không | Bỏ định nghĩa trùng tên — con, marker và insert — trong cùng một bước undo trước khi tạo mới (re-import idempotent) |

Kết quả: `{block, insert}` (handle của INSERT mới). REST: **route**
`POST /blocks`.

```json
{"op":"block_define","name":"MARK","base":[0,0,0],"handles":["63","64"]}
→ {"ok":true,"status":"completed","result":{"block":"MARK","insert":"69"},…}
```

#### `block_delete` — xóa một định nghĩa block

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `name` | string | **có** | Định nghĩa cần xóa — triệt để: entity con, block marker, table record, một bước undo (`Delete block <name>`) |
| `force` | boolean | không | Block đang được tham chiếu bị từ chối (`block_referenced`); `true` xóa luôn các INSERT đang tham chiếu trong cùng bước và báo lại số lượng |

Kết quả: `{block, erased, references}`. Tên lạ → `block_missing`; record
layout/anonymous/xref/dynamic → `block_protected`; phiên BEDIT đang mở →
`block_in_edit`; tên sai/trùng → `block_invalid_name`/`block_exists`.
REST: **route** `DELETE /blocks/{name}` (`?force=true` để đè qua tham chiếu).

```json
{"op":"block_delete","name":"MARK","force":true}
→ {"ok":true,"status":"completed","result":{"block":"MARK","erased":5,"references":1},…}
```

#### `wblock` — xuất một bản vẽ độc lập

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `path` | string | **có** | Đích `.dwg`/`.dxf` (đuôi file quyết định định dạng) |
| `handles` | string[] | một trong | Entity cần xuất — cần `handles` hoặc `block` |
| `block` | string | một trong | Một định nghĩa block nguyên vẹn để xuất |
| `template` | string | không | File gốc mà bản xuất thừa hưởng bảng/style |
| `normalize` | boolean | không | Dịch bản xuất để minimum bounds tổng thể rơi về gốc tọa độ |

Document nguồn không bị đụng tới. Kết quả: `{path, entities, normalized}`.
REST: **route** `POST /wblock`.

```json
{"op":"wblock","path":"C:/out/page-01.dwg","handles":["63","64"],"normalize":true}
→ {"ok":true,"status":"completed","result":{"path":"C:/out/page-01.dwg","entities":2,"normalized":true},…}
```

### 6.6 Layout và in ấn

#### `layout_create` — thêm một layout paper-space

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `name` | string | **có** | Tên tab layout mới; trùng bị từ chối (`layout_exists`) |

Layout khởi đầu với page setup mặc định và một viewport trang giấy.
REST: **route** `POST /layouts`.

```json
{"op":"layout_create","name":"SHEET-01"}
```

#### `page_setup_set` — lưu cấu hình in vào layout

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `layout` | string | **có** | Layout đích |
| `paper` | string | không | Tên khổ giấy canonical trong catalog, ví dụ `"ISO_A4_(210.00_x_297.00_MM)"` |
| `orientation` | enum | không | `Portrait` / `Landscape` |
| `fit` | boolean | không | Vừa khổ giấy (mặc định true) |
| `scale` | string | không | Tỷ lệ `"paper:drawing"` kiểu `"1:100"` — đè `fit` |
| `center` | boolean | không | Căn giữa trang (mặc định true) |
| `offset_x`, `offset_y` | number | không | Offset in tính bằng mm |
| `plot_style` | string | không | Đường dẫn CTB hoặc bảng plot style theo tên |
| `area` | enum | không | `extents`/`display`/`limits`/`layout`/`window` |
| `window` | number[4] | **khi area=window** | `[x0,y0,x1,y1]` |

Trường ghi tường minh thắng; phần còn lại giữ setup đã lưu — và setup đã lưu
chính là thứ `plot` dùng theo mặc định. REST: **route**
`PUT /layouts/{name}/page-setup`.

```json
{"op":"page_setup_set","layout":"SHEET-01","paper":"ISO_A4_(210.00_x_297.00_MM)",
 "orientation":"Landscape","scale":"1:100"}
```

#### `plot` — ghi PDF

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `path` | string | **có** | Đường dẫn đầu ra, phải kết thúc `.pdf` |
| `layout` | string | không | `"Model"` (mặc định), tên layout, hoặc `"all"` |
| `area` | enum | không | `extents` (mặc định), `display`, `limits`, `layout`, `window` |
| `window` | number[4] | **khi area=window** | `[x0,y0,x1,y1]` |
| `paper`, `orientation`, `center`, `offset_x`, `offset_y`, `plot_style` | — | không | Cùng ý nghĩa với `page_setup_set`; trường bỏ qua rơi về setup đã lưu |
| `fit` / `scale` | bool / string | không | `fit` mặc định true khi `scale` bị bỏ qua; tỷ lệ kiểu `"1:100"` đè nó |
| `upside_down` | boolean | không | Xoay nội dung 180° |
| `transparency`, `lineweights`, `merge_lines`, `stamp` | boolean | không | Công tắc đầu ra |
| `per_page` | boolean | không | Với `layout:"all"`: **mỗi layout một PDF**, đặt tên `<stem>-<Layout>.pdf` |

Chỉ PDF, đồng bộ, không hộp thoại thiết bị. Kết quả: `{path, pages,
page_sizes}` (khổ giấy tính bằng mm); với `per_page` → `{files, pages}`.
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

### 6.7 Metadata và biến hệ thống

**Extended data (`xdata_set` / `xdata_get`)** — record key-value có kiểu theo
tên application trên bất kỳ entity nào; mục bảng application được đăng ký
ngầm khi ghi. Danh sách `data` rỗng (hoặc vắng mặt) xóa record. Kết quả khi
ghi: `{updated, app}`. REST: **route** `PUT /entities/{handle}/xdata/{app}`
(body = mảng data) và `DELETE /entities/{handle}/xdata/{app}` để xóa.

```json
{"op":"xdata_set","handles":["65"],"app":"SPEC","data":[{"code":1000,"value":"PAGE-01"},{"code":1070,"value":3}]}
→ {"ok":true,"status":"completed","result":{"updated":1,"app":"SPEC"},…}
```

| Mã | Kiểu giá trị |
|---|---|
| `1000` | chuỗi |
| `1003` | tên layer |
| `1004` | byte hex |
| `1005` | handle hex |
| `1010`–`1013` | `[x,y,z]` điểm / vị trí / displacement / hướng |
| `1040` / `1041` / `1042` | số thực |
| `1070` / `1071` | số nguyên 16-bit / 32-bit |

**Biến hệ thống (`sysvar`)** — get/set nguyên tử trên một registry cố định:

| Biến | Ý nghĩa | Ghi được |
|---|---|---|
| `ltscale`, `celtscale` | hệ số linetype toàn cục / của entity hiện hành | ✓ |
| `clayer`, `ctextstyle` | layer / text style hiện hành (phải tồn tại) | ✓ |
| `textsize`, `filletrad`, `pdmode`, `pdsize` | mặc định cho nội dung mới | ✓ |
| `mirrtext` (`0`/`1`), `insunits`, `osmode` | cờ hành vi | ✓ |
| `extmin`, `extmax` | extents model-space `[x,y,z]` | chỉ đọc |

`{"op":"sysvar","set":{"ltscale":2.5}}` áp cả object nguyên tử
(`unknown_sysvar` / `invalid_sysvar_value` từ chối toàn bộ);
`{"op":"sysvar","get":["ltscale"]}` → `result.values`. Bỏ `get` trả mọi tên
đọc được. REST: **route** `GET /sysvars?names=ltscale,clayer` ·
`POST /sysvars`.

```json
{"op":"sysvar","get":["ltscale"]}
→ {"ok":true,"status":"completed","result":{"values":{"ltscale":2.5}},…}
```

**Sửa record (`set_properties`)** — ghi thuộc tính có kiểu, có kiểm tra hợp
lệ lên bất kỳ record cơ sở dữ liệu nào qua JSON Pointer paths RFC 6901, hỗ
trợ compare-and-set (`expected` trên một mục update sẽ từ chối khi giá trị
hiện tại khác); `records`/`record_schema` mô tả mọi trường ghi được.

```json
{"op":"set_properties","collection":"entities","handle":"64",
 "updates":[{"path":"/common/layer","value":"WALLS"},
            {"path":"/radius","value":12,"expected":10}]}
→ {"ok":true,"status":"completed",
   "result":{"collection":"entities","handle":"64","name":"Circle","paths":["/common/layer","/radius"],"changed":2},…}
```

### 6.8 Group và selection set

#### `group_create` — một group có tên trên các entity có sẵn

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `name` | string | **có** | Tên group |
| `handles` | string[] | **có** | Entity thành viên (phải tồn tại) |

Kết quả: `{group, handle}` — handle của chính đối tượng group trong cơ sở dữ
liệu. REST: **route** `POST /groups`.

```json
{"op":"group_create","name":"FASTENERS","handles":["6F","70"]}
→ {"ok":true,"status":"completed","result":{"group":"FASTENERS","handle":"88"},…}
```

#### `selection_set_save` — lưu một bộ handle có tên

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `name` | string | **có** | Tên bộ (theo session) |
| `handles` | string[] | **có** | Entity cần nhớ |

Kết quả: `{name, handles, count}`. REST: **route** `POST /selection-sets`.

```json
{"op":"selection_set_save","name":"TITLE-BLOCKS","handles":["6F","70"]}
→ {"ok":true,"status":"completed","result":{"name":"TITLE-BLOCKS","handles":["6F","70"],"count":2},…}
```

#### `selection_set_load` — gọi lại bộ đã lưu

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `name` | string | **có** | Tên bộ đã lưu (lạ → `selection_set_missing`) |
| `select` | boolean | không | Đồng thời biến nó thành selection hiện tại (mặc định true) |

Kết quả: `{name, handles, selected}`. REST: **route**
`GET /selection-sets/{name}?select=true`.

```json
{"op":"selection_set_load","name":"TITLE-BLOCKS"}
→ {"ok":true,"status":"completed","result":{"name":"TITLE-BLOCKS","handles":["6F","70"],"selected":2},…}
```

### 6.9 Ảnh — `embed_image`

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `path` | string | **có** | File ảnh (png/jpg) |
| `at` | Point | không | Góc dưới-trái theo tọa độ world |
| `width` | number | không | Bề rộng world theo đơn vị bản vẽ |
| `linked` | boolean | không | Lưu dạng `RasterImage` tham chiếu đường dẫn thay vì nhúng byte |
| `calibrate` | object | không | Hiệu chuẩn tham chiếu 2 điểm tự động: `{point_a:[px1,py1], point_b:[px2,py2], distance:<đơn vị bản vẽ>, align_to:[x,y]}` |
| `layer` | string | không | Layer đích (tự tạo khi thiếu, ví dụ `_XREF`) |
| `lock_layer` | boolean | không | Khóa layer đích sau khi gắn |
| `source_points` / `target_points` | Point[2] | không | Căn chỉnh 2 điểm tường minh: điểm pixel ảnh ↔ điểm world CAD |

| Chế độ | Hành vi |
|---|---|
| **Nhúng (mặc định)** | Byte ảnh được lưu bên trong bản vẽ thành một frame entity tự chứa. Không có đường dẫn ngoài nên ảnh không thể vỡ khi file nguồn bị chuyển hay xóa, và đi qua định nghĩa block, bản nhân bản và xuất như mọi entity. Đổi lấy: DWG phình đúng cỡ ảnh, và hình không bao giờ cập nhật khi file nguồn đổi. Dùng cho barcode/QR và mọi ảnh thuộc phần bàn giao. |
| `"linked":true` | Lưu ảnh tham chiếu theo đường dẫn. File phải đi cùng bản vẽ và *có thể* vỡ; chế độ tồn tại cho ảnh cần thay thế được mà không đụng bản vẽ. |

```json
{"op":"embed_image","path":"C:/img/qrcode.png","at":[180,10],"width":20}
→ {"ok":true,"status":"completed",
   "result":{"handle":"8C","kind":"RasterImage","path":"C:/img/qrcode.png",
             "at":[180,10,0],"width":20,"height":20,"scale":1.0,"linked":false},…}
```

Underlay hiệu chuẩn — scale và đặt một bản scan sao cho hai pixel đã biết
cách nhau đúng 6500 đơn vị bản vẽ, căn vào gốc tọa độ world:

```json
{"op":"embed_image","path":"C:/scan/plan.png","linked":true,
 "calibrate":{"point_a":[52,910],"point_b":[450,910],"distance":6500,"align_to":[0,0]},
 "layer":"_XREF","lock_layer":true}
```

Sửa ảnh linked vỡ từ bản vẽ do nơi khác soạn — không cần op riêng:

1. `query` với `type:"RasterImage"`, `detail:"full"` — entity lộ `file_path`
   và `bounds` world.
2. `entities_delete` tham chiếu vỡ theo handle.
3. `embed_image` bản thay thế tại cùng `at`/`width` suy ra từ bounds cũ —
   dạng nhúng, nên không thể vỡ lần nữa.

### 6.10 Lệnh, batch, điều khiển

#### `run` — chạy một dòng lệnh headless

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `cmd` | string | **có** | Tên lệnh + các câu trả lời prompt cách nhau bởi dấu cách (`LINE 0,0 10,10`); điểm `x,y[,z]`, option theo token hiển thị |

Cần một document đang mở. `status` của phản hồi là `completed` hoặc
`waiting_input` (lệnh muốn thêm — nuôi bằng `input` hoặc dứt điểm bằng
`cancel`). REST: **route** `POST /commands` với `{"cmd":…}`.

```json
{"op":"run","cmd":"LINE 0,0 10,10"}
→ {"ok":true,"status":"completed","result":{…},…}
```

#### `start` / `input` — hộp thoại lệnh tương tác

`start` mở một lệnh và giữ nó chờ; `state.command` khi đó báo `accepts[]`,
`options[]` và `input_example`. `input` nạp một bước:

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `kind` | enum | **có** | `text`, `token`, `point`, `entity`, `structure`, `selection`, `enter` |
| `text` | string | khi kind=`token` | Văn bản tự do hoặc option token |
| `point` | Point | khi kind=`point` | Điểm cần nạp |
| `space` | enum | không | `wcs` (mặc định), `ucs`, `relative` |
| `handle` | string | khi kind=`entity`/`structure` | Entity đích (cùng `point` làm vị trí pick) |

```json
{"op":"start","cmd":"CIRCLE"}
{"op":"input","kind":"point","point":[10,20]}
{"op":"input","kind":"token","text":"D"}
{"op":"input","kind":"text","text":"25"}
```

#### `cancel` — Escape

Không có trường. Hủy lệnh đang chạy — hoặc rút pick đang chờ của chính client
này (§6.12).

```json
{"op":"cancel"}
```

#### `batch` — nhiều op tuần tự trong một call

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `steps` | op[] | **có** | 1–64 thao tác; mỗi bước thấy trạng thái của bước trước; dừng tại lỗi đầu tiên và báo `completed_steps` / `next_step` |

```json
{"op":"batch","steps":[
  {"op":"entities_create","entities":[{"type":"Line","start":[0,0],"end":[10,0]}]},
  {"op":"block_define","name":"TICK","base":[0,0],"handles":["70"]}
]}
→ {"ok":true,"status":"completed","result":{"completed_steps":2},…}
```

#### `select` — đổi selection hiện tại

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `handles` | string[] | không | Các handle chọn tường minh |
| `type` / `layer` | string | không | Lọc loại entity / layer |
| `window` | number[4] | không | Hộp world `[x1,y1,x2,y2]`: chọn những gì nằm trong, như window của viewport |
| `crossing` | boolean | không | Với `window`: chọn thêm những gì hộp cắt qua |
| `clear` | boolean | không | Xóa selection có sẵn trước |
| `where` | filter[] | không | Cùng vị từ với `where` của `query` (§6.4) |

```json
{"op":"select","layer":"FRAME","clear":true}
```

#### `property` — đặt một thuộc tính cho các entity đang chọn

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `field` | string | **có** | Tên thuộc tính (`color`, `layer`, `linetype`, …) |
| `value` | bất kỳ | **có** | Giá trị mới (quy ước §2 — màu là chỉ số ACI) |

```json
{"op":"property","field":"color","value":1}
```

#### `view_focus` — zoom tới entity và làm nổi bật

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `handles` | string[] | **có** | Entity cần đóng khung |
| `highlight` | boolean | không | Đồng thời chọn chúng (mặc định true) |

Chỉ session GUI; headless trả `gui_required`. Kết quả: `{zoomed, selected}`.

```json
{"op":"view_focus","handles":["6F"]}
→ {"ok":true,"status":"completed","result":{"zoomed":true,"selected":1},…}
```

#### `action` — gọi một action UI theo tên

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `name` | string | **có** | Tên action đã đăng ký (công tắc grid/ortho/zoom và các hộp thoại — enum được quảng bá trong schema MCP, sai sẽ bị từ chối kèm danh sách tên) |

```json
{"op":"action","name":"zoom_extents"}
```

#### `operation` — poll một thao tác đang chờ

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `request_id` | string | **có** | Id của op cần poll (việc bất đồng bộ trả `status:"running"`, §5.1) |

```json
{"op":"operation","request_id":"rest-7"}
→ {"ok":true,"status":"completed","result":{…},…}
```

#### `stop` — công tắc ngắt

Không có trường. Vô hiệu hóa toàn bộ **mutation** automation của session cho
đến khi có người bật lại automation trong GUI desktop; mutation sau đó trả
`disabled`. Read vẫn hoạt động. Ý nghĩa: một integrator hành xử lệch lạc có
tự tắt mình mà không giết editor.

```json
{"op":"stop"}
→ {"ok":true,"status":"completed",…}
```

`events`, `history` và `capture` là read — hợp đồng đầy đủ ở §6.4.1.

### 6.11 Bảng endpoint REST đầy đủ

| Method & path | Mục đích |
|---|---|
| `GET /ready` · `/state` · `/capabilities` · `/openapi` | Sẵn sàng · trạng thái đầy đủ · quảng bá tính năng · tài liệu OpenAPI 3 |
| `GET /documents` · `POST /documents` · `DELETE /documents/{id}?discard=` | Liệt kê · mở/template/mới · đóng |
| `GET /sysvars?names=` · `POST /sysvars` | Đọc · đặt |
| `POST /layouts` · `PUT /layouts/{name}/page-setup` | Tạo · ghi cấu hình in |
| `GET /entities` · `GET /entities/{h}` · `POST /entities` · `DELETE /entities?handles=` | Truy vấn (gồm `where`) · một entity · tạo · xóa |
| `POST /entities/transform` · `POST /entities/copy-to` | Biến đổi · copy chéo document |
| `GET/PUT/DELETE /entities/{h}/xdata[/{app}]` | Extended data |
| `POST /blocks` · `DELETE /blocks/{name}` | Định nghĩa (+`replace`) · xóa |
| `POST /groups` · `POST /selection-sets` · `GET /selection-sets/{name}` | Group · selection set |
| `POST /getpoint` · `POST /user_select` | Pick có người tham gia (§6.12; kết nối giữ chờ) |
| `POST /plot` · `/wblock` · `/images` · `/commands` · `/undo` · `/redo` · `/save` | In, xuất, dòng lệnh, vòng đời |
| `GET /layers` · `/header` · `/records` | Đọc cơ sở dữ liệu |
| `POST /file-identity` | GUID ổn định của bản vẽ |
| `POST /{op}` | Passthrough cho **mọi** op của §6 — mutation lẫn read, gồm `text_replace`, `save_verified`, `stop`, mô phỏng viewport của §6.13, `operation` và `cancel` |

### 6.12 Pick human-in-the-loop — `user_select` & `getpoint`

Hai thao tác này trao màn hình cho người ngồi trước máy. Chúng chỉ được trả
lời ở nơi có con người: kênh GUI host (§3.2) hoặc phiên MCP gắn vào bản build
desktop; session headless trả `gui_required`. Cả hai đều giữ chờ — client
poll thấy `running` — cho tới khi người đó trả lời, và cả hai đều rút được
theo chương trình bằng `{"op":"cancel","request_id":"<một id mới>"}`.

**`user_select`** — lấy mẫu kiểu ssget. Người dùng chọn entity bằng cử chỉ
thường (click, window, crossing), **Enter** xác nhận, **Escape** hủy.

```json
{"protocol":1,"op":"user_select","request_id":"sample-1","document_id":1,
 "type":"INSERT","layer":"TITLE","prompt":"Chọn các block cần lấy props",
 "detail":"full","clear":true}
```

| Trường | Ý nghĩa |
|---|---|
| `type`, `layer` | bộ lọc tùy chọn cho cái gì tính là được chọn; so khớp không phân biệt hoa thường với tên hiển thị (`"Block Reference"`) và tên DXF (`"INSERT"`) |
| `prompt` | tùy chọn; thay câu gợi ý mặc định trên dòng lệnh |
| `detail` | `summary` / `geometry` / `full` (mặc định) — `full` trả toàn bộ thuộc tính đã serialize của từng entity |
| `clear` | mặc định `true`: request bắt đầu một selection mới; `false` giữ nguyên cái đang chọn |

Xác nhận xong resolves với:

```json
{"ok":true,"status":"completed","result":{
  "cancelled":false,"count":1,"ignored":1,
  "handles":["6F"],
  "entities":[{"handle":"6F","type":"Block Reference","block":"ANCHOR_A",
               "position":[0,0,0],"properties":{…}}]}}
```

`ignored` đếm các pick bị bộ lọc `type`/`layer` loại tại thời điểm xác nhận
(cử chỉ của người dùng không bao giờ bị chặn; chỉ câu trả lời bị lọc).
Escape — hoặc một lời rút `cancel`, hoặc document bị đóng — resolves với
`status:"cancelled"` và `{"cancelled":true,"count":0}`. Mở một pick khi lệnh
hoặc hộp thoại đang chạy bị từ chối (`user_input_busy`); pick thứ hai chạy
đồng thời bị từ chối (`interactive_pending`).

**`getpoint`** — một điểm world đã bắt từ cú click trái kế tiếp.

```json
{"protocol":1,"op":"getpoint","request_id":"qr-spot","document_id":1,
 "prompt":"Chọn vị trí đặt QR code"}
→ {"ok":true,"status":"completed","result":{"point":[125.5,64.25,0.0]}}
```

Escape hủy với `{"cancelled":true}`. `detail` không áp dụng.

**Người dùng thấy gì khi pick đang chờ** (để integrator biết request đã hiện
ra trước cả khi ai đó click):

- dòng lệnh ghim một dòng request có nhãn, không phai:
  `[user_select · sample-1] Chọn các block cần lấy props  (Enter confirms, Esc cancels)`;
- viên automation trên dòng lệnh chuyển xanh dương — *"MCP is waiting for
  you to pick — Enter confirms, Esc cancels"*;
- con trỏ viewport đổi chéo chữ thập lấy pickbox xanh;
- câu trả lời được in ngược vào danh sách thông điệp với cùng định danh
  (`user_select sample-1: handed 1 object(s) to the client.`), và toàn bộ
  vệt của mọi thao tác client — kể cả thất bại — luôn xem lại được trong
  lịch sử dòng lệnh.

Trên kênh HTTP GUI host (§3.2) toàn cuộc trao đổi đi trên một kết nối giữ
chờ: request nắm cho tới khi người đó trả lời (tối đa 30 phút, rồi
`response_timeout`), nên không cần vòng poll. Caller thích polling có thể coi
phản hồi `accepted` là tín hiệu đã nhận và bám theo thao tác bằng các read
`{"op":"operation","request_id":…}`; một pick giữ chờ mất kết nối (hoặc mất
caller) được rút tự động, và vệt mọi thay đổi trạng thái phía server — bắt
đầu, trả lời, hủy — đổ ra stderr dưới dạng dòng `[pick] <op> <request_id>: …`
để đối chiếu với log của client.

---

### 6.13 Mô phỏng viewport — `click`, `pointer`, `grip_drag`

Ba op này lái viewport đúng cách con chuột làm, cho các client cần tái tạo
cử chỉ của con người thay vì sửa cơ sở dữ liệu trực tiếp (lệnh tương tác cần
pick, kéo grip, chuỗi pan/zoom). Chúng chạy trên mọi session có viewport; mỗi
bước báo con trỏ đang thấy gì. Cả ba đi qua REST passthrough
(`POST /api/v1/{op}`).

#### `click` — một cú click trái tại điểm world

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `point` | Point | **có** | Điểm world cần click (chiếu qua camera sống) |
| `modifiers` | string | không | Phím giữ khi click: `"ctrl"`, `"shift"` hoặc `"ctrl+shift"` (Ctrl+click chọn solid của khối ghép dưới con trỏ) |

```json
{"op":"click","point":[10,0,0],"modifiers":"ctrl"}
```

#### `pointer` — phát lại một chuỗi sự kiện chuột

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `events` | event[] | **có** | Theo thứ tự; mỗi phần tử chứa đúng một trong `move` (pixel canvas `[x,y]`), `move_world` (world `[x,y,z]`, chiếu qua camera mà con trỏ đang sửa theo), `press` / `release` (`"left"`, `"right"`, `"middle"`), `wheel` (số dòng, dương zoom vào) |
| `modifiers` | string | không | Phím giữ suốt chuỗi |

Một chuỗi press→move→release đi cùng đường dẫn con trỏ như kéo tay, nên
window cao-su (rubber-band) và kéo grip hành xử y hệt. Mỗi bước báo điểm
world của con trỏ.

```json
{"op":"pointer","events":[
  {"move_world":[10,0,0]},{"press":"left"},
  {"move_world":[12,3,0]},
  {"release":"left"}
]}
```

#### `grip_drag` — kéo một grip tới điểm world

| Trường | Kiểu | Bắt buộc | Ý nghĩa |
|---|---|---|---|
| `handle` | string | **có** | Entity có grip cần kéo |
| `grip_id` | integer | **có** | Id grip từ read `grips` (§6.4.1) |
| `point` | Point | **có** | Điểm world grip được kéo tới |

Chạy đúng cách một cú pick-và-đặt grip, với object snap, grid, ortho, polar
và tracking tắt trong lúc kéo — grip hạ đúng trên `point`.

```json
{"op":"grip_drag","handle":"2A","grip_id":0,"point":[10,0,0]}
```

---

## 7. Ví dụ mẫu

### 7.1 Vẽ, kiểm chứng, xuất bản (REST)

```sh
# Tạo (layer thiếu tự tạo; cả loạt là nguyên tử).
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/entities -H "Content-Type: application/json" -d '{
  "entities":[
    {"type":"Line","start":[0,0],"end":[100,0],"layer":"FRAME"},
    {"type":"Text","value":"PAGE-01","position":[10,50],"height":3.0}
  ]}'
# → 201 {"result":{"handles":["63","64"],"created":2,"layers_created":["FRAME"]}}

# Lọc theo thuộc tính.
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" "http://127.0.0.1:8090/api/v1/entities?type=Line&where=%5B%7B%22path%22%3A%22%2Flayer%22%2C%22op%22%3A%22eq%22%2C%22value%22%3A%22FRAME%22%7D%5D"

# Đánh dấu, bọc thành block tái sử dụng, in mọi trang, lưu thành template.
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X PUT http://127.0.0.1:8090/api/v1/entities/64/xdata/SPEC \
  -H "Content-Type: application/json" -d '[{"code":1000,"value":"PAGE-01"}]'
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/blocks \
  -H "Content-Type: application/json" -d '{"name":"MARK","base":[0,0,0],"handles":["63","64"]}'
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/plot \
  -H "Content-Type: application/json" -d '{"path":"C:/out/pages.pdf","layout":"all","per_page":true}'
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/save \
  -H "Content-Type: application/json" -d '{"path":"C:/out/session.dwt"}'
```

### 7.2 Lấy mẫu entity khi có người tại màn hình (GUI host)

```sh
OpenCADStudio.exe "C:/drawings/plan.dwg" --http 8090   # editor vẫn tương tác bình thường

# Nhờ người dùng chọn block reference; kết nối giữ chờ tới khi họ bấm
# Enter (hoặc Esc). Không cần vòng poll.
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -m 1800 -X POST http://127.0.0.1:8090/api/v1/user_select \
  -H "Content-Type: application/json" \
  -d '{"request_id":"sample-1","type":"INSERT","detail":"full","prompt":"Pick the title blocks"}'
# → {"ok":true,"status":"completed","result":{"cancelled":false,"count":2,…}}

# Đọc lại cùng các entity, rồi tiếp tục lái bản vẽ đang sống.
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/get_selection -d '{}'
curl -s -H "Authorization: Bearer $OCS_API_TOKEN" -X POST http://127.0.0.1:8090/api/v1/entities/transform \
  -H "Content-Type: application/json" \
  -d '{"handles":["6F"],"action":"move","vector":[5,0]}'
```

### 7.3 Nhân bản một vùng thành bản vẽ mới đã normalize

```json
{"protocol":1,"op":"wblock","request_id":"clone-1","document_id":1,
 "handles":["63","64"],
 "template":"C:/tpl/base.dwt",
 "normalize":true,
 "path":"C:/out/page-01.dwg"}
```

Bản xuất thừa hưởng bảng/style của template và được dịch để minimum rơi về
gốc tọa độ. Mở nó sau đó bằng `{"op":"open","path":…}` và kiểm chứng bằng
`query`.

### 7.4 Định danh ổn định + retry idempotent

```json
{"protocol":1,"op":"file_identity","request_id":"fid-1","document_id":1}
→ {"ok":true,"result":{"identity":"5db78b90-ca0b-4a4f-ad68-8c860483a41f","created":true}}
{"protocol":1,"op":"file_identity","request_id":"fid-1","document_id":1}
→ cùng phản hồi được phục vụ từ cache idempotency — không bao giờ thực thi hai lần
```

---

## 8. Trạng thái bề mặt và bản đồ upstream

Spec này mô tả bề mặt của fork, là tập hợp cha thực sự của upstream `main`.
Mọi thứ trong cột **core** đều đã tồn tại trên upstream hiện nay và được tài
liệu hóa ở đây nguyên văn; mỗi dòng **piece** là một bổ sung tự chứa, review
độc lập được — mục spec tương ứng được thiết kế đi cùng PR của piece đó, nên
không mục nào chạy trước code mà nó mô tả.

| Phạm vi | Nội dung | Mục spec |
|---|---|---|
| **Core (upstream `main`)** | Transport `--serve` (stdio JSONL), `--serve --port` (TCP), `--mcp`; op `new`, `open`, `save`, `save_verified`, `activate`, `switch_document`, `select`, `run`, `start`, `input`, `cancel`, `stop`, `action`, `property`, `set_properties`, `text_replace`, `embed_image`, `capture`, `undo`, `redo`, `grip_drag`, `click`, `pointer`; read `state`, `hello`, `capabilities`, `query`/`entities`, `records`, `record_schema`, `entity_schema`, `layers`, `header`, `properties`, `measure`, `snap`, `grips`, `xdata_get`, `get_selection`, `text_search`, `text_audit`, `audit`, `events`, `history`; quy ước envelope và các mã lỗi lõi | §1–§5, §6.1, §6.4, §6.10, §6.13 |
| Piece A — entity CRUD | `entities_create`, `entities_delete`, `entities_transform`, `entities_copy_to` | §6.2, §6.3 |
| Piece B — block & xuất | `block_define`, `block_delete`, `wblock` | §6.5 |
| Piece C — in ấn | `layout_create`, `page_setup_set`, `plot` (gồm `per_page`) | §6.6 |
| Piece D — metadata | `sysvar`, `xdata_set`/`xdata_get`, `file_identity` | §6.7 |
| Piece E — collection | `group_create`, `selection_set_save`/`load` | §6.8 |
| Piece F — mặt điều khiển | `batch`, `events`, `view_focus`, `close`, bộ lọc `where` | §6.4 (`where`), §6.10 |
| Piece G — transport REST | server headless `--http`, route `/api/v1`, tài liệu OpenAPI 3 | §3.1, §6.11, §7.1 |
| Piece H — người trong vòng lặp | kênh GUI host `--http`, `user_select`, `getpoint`, các tín hiệu chờ trên màn hình | §3.2, §6.12, §7.2 |

Các mục §3.2/§6.12 (piece H) cần thêm bản build desktop; trên session headless
mọi request đã tài liệu hóa vẫn trả lời trung thực (`gui_required`), nên
client viết theo spec này hành xử nhất quán trên cả hai.

---

## 9. Phiên bản và tính tương thích

- Phiên bản giao thức dây truyền là **1**; được khai báo trong mọi envelope và
  trong dòng chào. Các bổ sung (op mới, trường tùy chọn mới, mã lỗi mới) không
  phá vỡ (non-breaking); client nên bỏ qua trường lạ và coi tên op lạ trong
  `capabilities` là "không có trong bản build này".
- Dò tính năng: đọc `capabilities` trước công việc xa lạ — mọi op trong tài
  liệu này được liệt kê ở đó khi bản build đang chạy hỗ trợ.
- File bản vẽ: bộ đọc/ghi DWG và DXF chịu đựng phiên bản; template (`*.dwt`)
  là file DWG tương thích từng byte.

*Tài liệu đi kèm: hướng dẫn thực hành automation và cài đặt MCP
([`README.md`](README.md)) và mô tả OpenAPI 3 phục vụ tại
`GET /api/v1/openapi`.*
