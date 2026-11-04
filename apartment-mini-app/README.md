# CưDân Hub — Mini app cho cư dân chung cư

Ứng dụng di động (web app / mini app) kết nối **Ban quản lý (BQL)**, **cư dân** và **cửa hàng nội khu** trong một toà chung cư:

| Nhóm tính năng | Cư dân | Chủ cửa hàng | Ban quản lý |
| --- | --- | --- | --- |
| **Thông tin** | Xem thông tin toà nhà, tiện ích, hotline (bấm để gọi), thông báo theo danh mục | — | Đăng / ghim / xoá thông báo, cập nhật thông tin toà nhà & hotline (API) |
| **Đơn hàng** | Duyệt cửa hàng, tìm kiếm không dấu, giỏ hàng, đặt hàng giao tận căn, theo dõi & huỷ đơn | Nhận đơn, xác nhận → giao → hoàn thành, quản lý sản phẩm & tình trạng còn hàng | Xem & điều phối mọi đơn |
| **Quảng bá** | Xem ưu đãi đang chạy ở trang chủ / trang Ưu đãi | Tạo tin quảng bá (chờ BQL duyệt) | Duyệt / từ chối tin, đăng tin của BQL (hiển thị ngay) |
| **Đánh giá** | Chấm sao + nhận xét sau khi đơn **hoàn thành** (mỗi đơn 1 lần — đánh giá thật) | Phản hồi đánh giá | — |

## Công nghệ

- **Frontend**: React 19 + TypeScript + Vite 7, React Router 7. Giao diện mobile-first, tiếng Việt, hỗ trợ chế độ tối.
- **Backend**: Node.js ≥ 22.13 + Express 5 + TypeScript, xác thực JWT, kiểm tra dữ liệu bằng zod.
- **Database**: SQLite qua module có sẵn `node:sqlite` — không cần cài thêm DB hay native module.
- Một tiến trình duy nhất phục vụ cả API (`/api`) và giao diện khi triển khai.

```
apartment-mini-app/
├── backend/
│   ├── src/
│   │   ├── app.ts            # Ghép router, phục vụ SPA
│   │   ├── auth.ts           # Băm mật khẩu (scrypt), JWT, phân quyền
│   │   ├── db/               # Schema + migration, dữ liệu mẫu
│   │   └── routes/           # auth, building, announcements, shops, orders, promotions, reviews
│   └── test/api.test.ts      # 22 test tích hợp cho toàn bộ API
├── frontend/
│   └── src/
│       ├── pages/            # Trang chủ, Thông tin, Cửa hàng, Giỏ hàng, Đơn hàng, Tài khoản, merchant/, admin/
│       ├── components/       # UI dùng chung
│       └── state/            # Đăng nhập, giỏ hàng, thông báo toast
└── scripts/publish-to-gitea.sh
```

## Chạy thử

```bash
npm install
npm run dev          # API: http://localhost:4000, giao diện: http://localhost:5173
```

Lần chạy đầu, backend tự tạo `backend/data/cudan.db` và nạp dữ liệu mẫu. Ở trang **Đăng nhập** có nút điền sẵn tài khoản demo:

| Vai trò | Số điện thoại | Mật khẩu |
| --- | --- | --- |
| Cư dân | 0900000010 | cudan123 |
| Cư dân | 0900000011 | cudan123 |
| Chủ cửa hàng (Bếp Nhà Lan, Giặt Sấy) | 0900000002 | shop123 |
| Chủ cửa hàng (Trà Sữa Mây, Tạp Hoá) | 0900000003 | shop123 |
| Ban quản lý | 0900000001 | admin123 |

Các lệnh khác:

```bash
npm test             # chạy test API
npm run typecheck    # kiểm tra kiểu cả backend lẫn frontend
npm run build        # build backend (dist/) và frontend (frontend/dist/)
npm start            # chạy bản build: API + giao diện tại http://localhost:4000
```

## Cấu hình (backend/.env)

Xem `backend/.env.example`.

| Biến | Mặc định | Ý nghĩa |
| --- | --- | --- |
| `PORT` | `4000` | Cổng HTTP |
| `DATABASE_PATH` | `./data/cudan.db` | File SQLite (`:memory:` để chạy tạm) |
| `JWT_SECRET` | *(bắt buộc khi production)* | Khoá ký token đăng nhập |
| `SEED_DEMO` | `true` (dev) / `false` (production) | Nạp dữ liệu mẫu khi DB trống |
| `ADMIN_PHONE`, `ADMIN_PASSWORD` | — | Tạo tài khoản BQL đầu tiên khi chưa có admin |
| `CORS_ORIGIN` | — | Danh sách origin được gọi API, phân tách bằng dấu phẩy (khi frontend chạy ở domain khác) |
| `STATIC_DIR` | `../frontend/dist` | Thư mục giao diện đã build |

Triển khai thật:

```bash
npm ci && npm run build
NODE_ENV=production JWT_SECRET=<chuỗi-ngẫu-nhiên-dài> ADMIN_PHONE=09xxxxxxxx ADMIN_PASSWORD=<mật-khẩu> npm start
```

Tài khoản **Ban quản lý** không tự đăng ký được; cư dân và chủ cửa hàng tự đăng ký trong app.

## API

Mọi endpoint nằm dưới `/api`, trả JSON. Gửi token bằng header `Authorization: Bearer <token>`.

| Method | Đường dẫn | Quyền | Mô tả |
| --- | --- | --- | --- |
| POST | `/auth/register` | — | Đăng ký (`role`: `resident` \| `merchant`) |
| POST | `/auth/login` | — | Đăng nhập, trả `{ token, user }` |
| GET / PATCH | `/auth/me` | đăng nhập | Xem / sửa hồ sơ (họ tên, căn hộ) |
| GET | `/building` | — | Thông tin toà nhà, tiện ích, hotline |
| PUT | `/building` | BQL | Cập nhật thông tin & danh sách hotline |
| GET | `/announcements?category=` | — | Danh sách thông báo (tin ghim lên đầu) |
| POST / PATCH / DELETE | `/announcements/:id` | BQL | Quản lý thông báo |
| GET | `/shops?category=&q=` | — | Cửa hàng đang mở + điểm đánh giá |
| GET | `/shops/mine` | chủ shop | Cửa hàng của tôi |
| GET | `/shops/:id` | — | Chi tiết + sản phẩm |
| POST / PATCH | `/shops`, `/shops/:id` | chủ shop | Mở / sửa cửa hàng |
| POST | `/shops/:id/products` | chủ shop | Thêm sản phẩm |
| PATCH / DELETE | `/products/:id` | chủ shop | Sửa giá, ẩn/hiện, xoá sản phẩm |
| POST | `/orders` | đăng nhập | Tạo đơn (server tự tính tiền theo giá hiện tại) |
| GET | `/orders?as=resident\|merchant&status=` | đăng nhập | Đơn của tôi / đơn của cửa hàng tôi |
| GET | `/orders/:id` | người đặt, chủ shop | Chi tiết, kèm `allowedStatuses` và `canReview` |
| PATCH | `/orders/:id/status` | theo luồng | `pending → confirmed → delivering → completed`; cư dân chỉ huỷ được khi `pending` |
| GET | `/promotions` | — | Tin đã duyệt, còn hiệu lực |
| GET | `/promotions/manage` | chủ shop, BQL | Tin của tôi / tất cả tin (để duyệt) |
| POST | `/promotions` | chủ shop, BQL | Tạo tin (của shop: chờ duyệt; của BQL: đăng ngay) |
| PATCH | `/promotions/:id/status` | BQL | Duyệt / từ chối |
| DELETE | `/promotions/:id` | chủ shop, BQL | Xoá tin |
| GET | `/shops/:id/reviews` | — | Đánh giá + phân bố số sao |
| POST | `/orders/:id/review` | người đặt | Đánh giá đơn đã hoàn thành (1 lần) |
| POST | `/reviews/:id/reply` | chủ shop | Phản hồi đánh giá |

## Đưa lên Gitea

Script `scripts/publish-to-gitea.sh` tạo repo qua Gitea API rồi đẩy mã lên nhánh `main`. Nếu dự án đang nằm trong thư mục con của một repo khác, script dùng `git subtree split` để repo mới chỉ chứa dự án này (giữ nguyên lịch sử commit của thư mục).

```bash
# Token: Gitea → Settings → Applications → Generate New Token (quyền write:repository)
GITEA_URL=https://git.example.com GITEA_TOKEN=xxxx ./scripts/publish-to-gitea.sh
# Tạo trong tổ chức, đặt tên khác:
GITEA_URL=... GITEA_TOKEN=... GITEA_OWNER=ten-to-chuc REPO_NAME=cudan-hub ./scripts/publish-to-gitea.sh
```

Token chỉ được gửi qua HTTP header, không bị lưu vào cấu hình git.

## Hướng phát triển tiếp

- Đóng gói thành **Zalo Mini App** (dùng `zmp-ui` / `zmp-sdk`, đăng nhập bằng Zalo, thông báo qua OA) — frontend đã tách `VITE_API_URL`, backend có sẵn `CORS_ORIGIN`.
- Tải ảnh sản phẩm / ảnh quảng bá (trường `imageUrl` đã có trong API).
- Thanh toán online (VietQR, MoMo, ZaloPay) và đối soát chuyển khoản.
- Thông báo đẩy khi đơn đổi trạng thái hoặc BQL đăng thông báo mới.
- Phản ánh / yêu cầu sửa chữa gửi BQL, đặt lịch tiện ích (BBQ, sân thể thao).
- Giới hạn tần suất đăng nhập (rate limit) và nhật ký thao tác của BQL.
