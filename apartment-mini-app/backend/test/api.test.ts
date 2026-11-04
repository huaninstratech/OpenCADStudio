import assert from "node:assert/strict";
import { once } from "node:events";
import type { Server } from "node:http";
import type { AddressInfo } from "node:net";
import { after, before, describe, test } from "node:test";
import { createApp } from "../src/app.js";
import { loadConfig } from "../src/config.js";
import { openDatabase, type DB } from "../src/db/index.js";
import { DEMO_ACCOUNTS, seedDemoData } from "../src/db/seed.js";

let db: DB;
let server: Server;
let base: string;

type Json = any; // eslint-disable-line @typescript-eslint/no-explicit-any

async function call(
  method: string,
  path: string,
  { token, body }: { token?: string; body?: unknown } = {},
): Promise<{ status: number; body: Json }> {
  const res = await fetch(base + path, {
    method,
    headers: {
      ...(body !== undefined ? { "content-type": "application/json" } : {}),
      ...(token ? { authorization: `Bearer ${token}` } : {}),
    },
    body: body !== undefined ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  return { status: res.status, body: text ? JSON.parse(text) : null };
}

async function login(account: { phone: string; password: string }): Promise<string> {
  const res = await call("POST", "/auth/login", { body: account });
  assert.equal(res.status, 200, JSON.stringify(res.body));
  return res.body.token;
}

const tokens: Record<keyof typeof DEMO_ACCOUNTS, string> = {} as never;

before(async () => {
  db = openDatabase(":memory:");
  seedDemoData(db);
  const config = loadConfig({ NODE_ENV: "test", JWT_SECRET: "test-secret", STATIC_DIR: "/nonexistent" });
  server = createApp(db, config).listen(0);
  await once(server, "listening");
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}/api`;
  for (const key of Object.keys(DEMO_ACCOUNTS) as (keyof typeof DEMO_ACCOUNTS)[]) {
    tokens[key] = await login(DEMO_ACCOUNTS[key]);
  }
});

after(() => {
  server.close();
  db.close();
});

describe("hệ thống", () => {
  test("health check và API không tồn tại", async () => {
    assert.deepEqual((await call("GET", "/health")).body, { ok: true });
    assert.equal((await call("GET", "/khong-co")).status, 404);
  });

  test("body JSON sai cú pháp trả 400", async () => {
    const res = await fetch(`${base}/auth/login`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: "{oops",
    });
    assert.equal(res.status, 400);
  });
});

describe("tài khoản", () => {
  test("đăng ký cư dân, chặn trùng số điện thoại và dữ liệu sai", async () => {
    const body = { phone: "0912345678", password: "matkhau1", fullName: "Cư Dân Mới", apartment: "C-05.01" };
    const created = await call("POST", "/auth/register", { body });
    assert.equal(created.status, 201);
    assert.equal(created.body.user.role, "resident");
    assert.equal(created.body.user.apartment, "C-05.01");
    assert.ok(created.body.token);
    assert.equal(created.body.user.password_hash, undefined);

    assert.equal((await call("POST", "/auth/register", { body })).status, 409);

    const badPhone = await call("POST", "/auth/register", { body: { ...body, phone: "123" } });
    assert.equal(badPhone.status, 400);
    assert.equal(badPhone.body.error, "Số điện thoại không hợp lệ");
  });

  test("không thể tự đăng ký tài khoản BQL", async () => {
    const res = await call("POST", "/auth/register", {
      body: { phone: "0912345679", password: "matkhau1", fullName: "Hacker", role: "admin" },
    });
    assert.equal(res.status, 400);
  });

  test("đăng nhập sai mật khẩu và token không hợp lệ", async () => {
    const wrong = await call("POST", "/auth/login", {
      body: { phone: DEMO_ACCOUNTS.residentAn.phone, password: "sai-mat-khau" },
    });
    assert.equal(wrong.status, 401);
    assert.equal((await call("GET", "/auth/me")).status, 401);
    assert.equal((await call("GET", "/auth/me", { token: "abc.def.ghi" })).status, 401);
  });

  test("cập nhật hồ sơ", async () => {
    const res = await call("PATCH", "/auth/me", {
      token: tokens.residentBinh,
      body: { apartment: "B-08.12" },
    });
    assert.equal(res.status, 200);
    assert.equal(res.body.apartment, "B-08.12");
    assert.equal(res.body.fullName, DEMO_ACCOUNTS.residentBinh.fullName);
  });
});

describe("thông tin chung cư & thông báo", () => {
  test("xem thông tin chung cư và hotline", async () => {
    const res = await call("GET", "/building");
    assert.equal(res.status, 200);
    assert.match(res.body.name, /Hoa Sen/);
    assert.ok(res.body.contacts.length >= 3);
    assert.ok(Array.isArray(res.body.amenities));
  });

  test("chỉ BQL được cập nhật thông tin chung cư", async () => {
    const body = {
      name: "Hoa Sen Residence",
      address: "88 Đường Số 9",
      amenities: ["Hồ bơi"],
      contacts: [{ name: "BQL", phone: "02838000001" }],
    };
    assert.equal((await call("PUT", "/building", { token: tokens.residentAn, body })).status, 403);
    const res = await call("PUT", "/building", { token: tokens.admin, body });
    assert.equal(res.status, 200);
    assert.equal(res.body.contacts.length, 1);
    assert.deepEqual(res.body.amenities, ["Hồ bơi"]);
  });

  test("thông báo ghim nằm đầu danh sách", async () => {
    const res = await call("GET", "/announcements");
    assert.equal(res.status, 200);
    assert.equal(res.body[0].pinned, true);
    const fees = await call("GET", "/announcements?category=fee");
    assert.ok(fees.body.every((a: Json) => a.category === "fee"));
  });

  test("BQL tạo/sửa/xoá thông báo, cư dân thì không", async () => {
    const body = { title: "Bảo trì thang máy", content: "Thang số 2 bảo trì sáng thứ Hai.", category: "maintenance", pinned: true };
    assert.equal((await call("POST", "/announcements", { token: tokens.residentAn, body })).status, 403);

    const created = await call("POST", "/announcements", { token: tokens.admin, body });
    assert.equal(created.status, 201);
    assert.equal(created.body.authorName, DEMO_ACCOUNTS.admin.fullName);

    // PATCH một phần không được làm mất các trường còn lại.
    const patched = await call("PATCH", `/announcements/${created.body.id}`, {
      token: tokens.admin,
      body: { title: "Bảo trì thang máy số 2" },
    });
    assert.equal(patched.status, 200);
    assert.equal(patched.body.title, "Bảo trì thang máy số 2");
    assert.equal(patched.body.pinned, true);
    assert.equal(patched.body.category, "maintenance");

    assert.equal((await call("DELETE", `/announcements/${created.body.id}`, { token: tokens.admin })).status, 204);
    assert.equal((await call("GET", `/announcements/${created.body.id}`)).status, 404);
  });
});

describe("cửa hàng & sản phẩm", () => {
  test("tìm kiếm không dấu", async () => {
    const res = await call("GET", "/shops?q=bep nha");
    assert.equal(res.status, 200);
    assert.deepEqual(res.body.map((s: Json) => s.name), ["Bếp Nhà Lan"]);
  });

  test("lọc theo danh mục và xem chi tiết", async () => {
    const drinks = await call("GET", "/shops?category=drink");
    assert.ok(drinks.body.length > 0 && drinks.body.every((s: Json) => s.category === "drink"));

    const detail = await call("GET", `/shops/${drinks.body[0].id}`);
    assert.equal(detail.status, 200);
    assert.ok(detail.body.products.length > 0);
    assert.equal(detail.body.canManage, false);
  });

  test("chủ cửa hàng quản lý sản phẩm; người khác bị chặn", async () => {
    const [shop] = (await call("GET", "/shops/mine", { token: tokens.merchantLan })).body;
    const created = await call("POST", `/shops/${shop.id}/products`, {
      token: tokens.merchantLan,
      body: { name: "Bánh flan", price: 12000 },
    });
    assert.equal(created.status, 201);
    assert.equal(created.body.isAvailable, true);

    const other = await call("PATCH", `/products/${created.body.id}`, {
      token: tokens.merchantMay,
      body: { price: 1 },
    });
    assert.equal(other.status, 403);

    const hidden = await call("PATCH", `/products/${created.body.id}`, {
      token: tokens.merchantLan,
      body: { isAvailable: false },
    });
    assert.equal(hidden.body.isAvailable, false);
    assert.equal(hidden.body.price, 12000);

    // Cư dân không thấy sản phẩm đã ẩn, chủ shop vẫn thấy.
    const publicView = await call("GET", `/shops/${shop.id}`);
    assert.ok(!publicView.body.products.some((p: Json) => p.id === created.body.id));
    const ownerView = await call("GET", `/shops/${shop.id}`, { token: tokens.merchantLan });
    assert.ok(ownerView.body.products.some((p: Json) => p.id === created.body.id));

    assert.equal((await call("DELETE", `/products/${created.body.id}`, { token: tokens.merchantLan })).status, 204);
  });

  test("cư dân không được mở cửa hàng", async () => {
    const res = await call("POST", "/shops", {
      token: tokens.residentAn,
      body: { name: "Shop lậu", category: "food" },
    });
    assert.equal(res.status, 403);
  });
});

describe("đơn hàng & đánh giá", () => {
  let shopId: number;
  let productIds: number[];

  before(async () => {
    const [shop] = (await call("GET", "/shops/mine", { token: tokens.merchantMay })).body;
    shopId = shop.id;
    const detail = await call("GET", `/shops/${shopId}`);
    productIds = detail.body.products.map((p: Json) => p.id);
  });

  test("tạo đơn: gộp dòng trùng và tự tính tổng tiền ở server", async () => {
    const detail = (await call("GET", `/shops/${shopId}`)).body;
    const [p1, p2] = detail.products;
    const res = await call("POST", "/orders", {
      token: tokens.residentBinh,
      body: {
        shopId,
        items: [
          { productId: p1.id, quantity: 1 },
          { productId: p2.id, quantity: 2 },
          { productId: p1.id, quantity: 1 },
        ],
        note: "Giao trước 7h",
      },
    });
    assert.equal(res.status, 201, JSON.stringify(res.body));
    assert.equal(res.body.status, "pending");
    assert.equal(res.body.items.length, 2);
    assert.equal(res.body.total, p1.price * 2 + p2.price * 2);
    // Mặc định giao về căn hộ trong hồ sơ.
    assert.equal(res.body.deliveryApartment, "B-08.12");
  });

  test("từ chối sản phẩm của cửa hàng khác và giỏ hàng trống", async () => {
    const otherShopProduct = (await call("GET", "/shops?category=food")).body[0];
    const foreign = (await call("GET", `/shops/${otherShopProduct.id}`)).body.products[0];
    const res = await call("POST", "/orders", {
      token: tokens.residentAn,
      body: { shopId, items: [{ productId: foreign.id, quantity: 1 }] },
    });
    assert.equal(res.status, 400);

    const empty = await call("POST", "/orders", { token: tokens.residentAn, body: { shopId, items: [] } });
    assert.equal(empty.status, 400);
    assert.equal(empty.body.error, "Giỏ hàng trống");
  });

  test("luồng trạng thái đơn hàng và đánh giá", async () => {
    const order = (
      await call("POST", "/orders", {
        token: tokens.residentAn,
        body: { shopId, items: [{ productId: productIds[0], quantity: 1 }] },
      })
    ).body;
    const setStatus = (token: string, status: string) =>
      call("PATCH", `/orders/${order.id}/status`, { token, body: { status } });

    // Mỗi phía chỉ thấy thao tác của mình.
    assert.deepEqual(order.allowedStatuses, ["cancelled"]);
    assert.equal(order.viewerIsShop, false);
    const shopView = (await call("GET", `/orders/${order.id}`, { token: tokens.merchantMay })).body;
    assert.deepEqual(shopView.allowedStatuses, ["confirmed", "cancelled"]);
    assert.equal(shopView.viewerIsShop, true);

    // Chủ shop khác không nhìn thấy đơn.
    assert.equal((await call("GET", `/orders/${order.id}`, { token: tokens.merchantLan })).status, 404);
    // Cư dân không thể tự xác nhận; không nhảy cóc trạng thái.
    assert.equal((await setStatus(tokens.residentAn, "confirmed")).status, 400);
    assert.equal((await setStatus(tokens.merchantMay, "completed")).status, 400);

    // Chưa hoàn thành thì chưa được đánh giá.
    const early = await call("POST", `/orders/${order.id}/review`, {
      token: tokens.residentAn,
      body: { rating: 5 },
    });
    assert.equal(early.status, 400);

    assert.equal((await setStatus(tokens.merchantMay, "confirmed")).body.status, "confirmed");
    // Đã xác nhận thì cư dân không huỷ được nữa.
    assert.equal((await setStatus(tokens.residentAn, "cancelled")).status, 400);
    assert.equal((await setStatus(tokens.merchantMay, "delivering")).body.status, "delivering");
    assert.equal((await setStatus(tokens.merchantMay, "completed")).body.status, "completed");
    const completed = (await call("GET", `/orders/${order.id}`, { token: tokens.residentAn })).body;
    assert.equal(completed.canReview, true);
    assert.deepEqual(completed.allowedStatuses, []);

    const merchantList = await call("GET", "/orders?as=merchant", { token: tokens.merchantMay });
    assert.ok(merchantList.body.some((o: Json) => o.id === order.id));
    assert.equal((await call("GET", "/orders?as=merchant", { token: tokens.residentAn })).status, 403);

    const before = (await call("GET", `/shops/${shopId}/reviews`)).body;

    // Người khác không đánh giá hộ được.
    const stranger = await call("POST", `/orders/${order.id}/review`, {
      token: tokens.residentBinh,
      body: { rating: 1 },
    });
    assert.equal(stranger.status, 404);

    const review = await call("POST", `/orders/${order.id}/review`, {
      token: tokens.residentAn,
      body: { rating: 5, comment: "Rất ngon!" },
    });
    assert.equal(review.status, 201);
    assert.equal(review.body.residentName, DEMO_ACCOUNTS.residentAn.fullName);

    const duplicate = await call("POST", `/orders/${order.id}/review`, {
      token: tokens.residentAn,
      body: { rating: 3 },
    });
    assert.equal(duplicate.status, 409);

    const afterStats = (await call("GET", `/shops/${shopId}/reviews`)).body;
    assert.equal(afterStats.total, before.total + 1);
    assert.equal(afterStats.items[0].comment, "Rất ngon!");
    assert.equal(afterStats.distribution.find((d: Json) => d.rating === 5).count, before.distribution[4].count + 1);

    const reviewedOrder = (await call("GET", `/orders/${order.id}`, { token: tokens.residentAn })).body;
    assert.deepEqual(reviewedOrder.review, { id: review.body.id, rating: 5 });
    assert.equal(reviewedOrder.canReview, false);

    // Phản hồi đánh giá: chỉ chủ cửa hàng.
    const replyPath = `/reviews/${review.body.id}/reply`;
    assert.equal((await call("POST", replyPath, { token: tokens.merchantLan, body: { reply: "x" } })).status, 403);
    const reply = await call("POST", replyPath, { token: tokens.merchantMay, body: { reply: "Cảm ơn anh!" } });
    assert.equal(reply.status, 200);
    assert.equal(reply.body.reply, "Cảm ơn anh!");
    assert.ok(reply.body.repliedAt);
  });

  test("cư dân huỷ đơn khi còn chờ xác nhận", async () => {
    const order = (
      await call("POST", "/orders", {
        token: tokens.residentAn,
        body: { shopId, items: [{ productId: productIds[1], quantity: 3 }], paymentMethod: "transfer" },
      })
    ).body;
    assert.equal(order.paymentMethod, "transfer");
    const res = await call("PATCH", `/orders/${order.id}/status`, {
      token: tokens.residentAn,
      body: { status: "cancelled" },
    });
    assert.equal(res.body.status, "cancelled");
  });

  test("không đặt được sản phẩm đã hết hàng", async () => {
    await call("PATCH", `/products/${productIds[2]}`, { token: tokens.merchantMay, body: { isAvailable: false } });
    const res = await call("POST", "/orders", {
      token: tokens.residentAn,
      body: { shopId, items: [{ productId: productIds[2], quantity: 1 }] },
    });
    assert.equal(res.status, 400);
    assert.match(res.body.error, /hết hàng/);
    await call("PATCH", `/products/${productIds[2]}`, { token: tokens.merchantMay, body: { isAvailable: true } });
  });
});

describe("quảng bá", () => {
  const range = () => ({
    startsAt: new Date(Date.now() - 60_000).toISOString(),
    endsAt: new Date(Date.now() + 7 * 86_400_000).toISOString(),
  });

  test("tin của cửa hàng chờ BQL duyệt mới hiển thị", async () => {
    const [shop] = (await call("GET", "/shops/mine", { token: tokens.merchantLan })).body;
    const created = await call("POST", "/promotions", {
      token: tokens.merchantLan,
      body: { shopId: shop.id, title: "Combo trưa 39k", discountLabel: "-15%", ...range() },
    });
    assert.equal(created.status, 201);
    assert.equal(created.body.status, "pending");

    const isPublic = async () =>
      (await call("GET", "/promotions")).body.some((p: Json) => p.id === created.body.id);
    assert.equal(await isPublic(), false);

    const managed = await call("GET", "/promotions/manage", { token: tokens.merchantLan });
    assert.ok(managed.body.some((p: Json) => p.id === created.body.id));

    assert.equal(
      (await call("PATCH", `/promotions/${created.body.id}/status`, { token: tokens.merchantLan, body: { status: "approved" } })).status,
      403,
    );
    const approved = await call("PATCH", `/promotions/${created.body.id}/status`, {
      token: tokens.admin,
      body: { status: "approved" },
    });
    assert.equal(approved.body.status, "approved");
    assert.equal(await isPublic(), true);

    assert.equal((await call("DELETE", `/promotions/${created.body.id}`, { token: tokens.merchantMay })).status, 403);
    assert.equal((await call("DELETE", `/promotions/${created.body.id}`, { token: tokens.merchantLan })).status, 204);
  });

  test("không quảng bá hộ cửa hàng khác; kiểm tra ngày", async () => {
    const [shop] = (await call("GET", "/shops/mine", { token: tokens.merchantLan })).body;
    const foreign = await call("POST", "/promotions", {
      token: tokens.merchantMay,
      body: { shopId: shop.id, title: "Giả mạo", ...range() },
    });
    assert.equal(foreign.status, 403);

    const { startsAt, endsAt } = range();
    const badDates = await call("POST", "/promotions", {
      token: tokens.merchantLan,
      body: { shopId: shop.id, title: "Sai ngày", startsAt: endsAt, endsAt: startsAt },
    });
    assert.equal(badDates.status, 400);
    assert.equal(badDates.body.error, "Ngày kết thúc phải sau ngày bắt đầu");
  });

  test("tin của BQL được đăng ngay", async () => {
    const res = await call("POST", "/promotions", {
      token: tokens.admin,
      body: { title: "Giải chạy cư dân", ...range() },
    });
    assert.equal(res.body.status, "approved");
    assert.equal(res.body.shopId, null);
  });
});
