import { Router } from "express";
import { z } from "zod";
import { getShopRecord } from "../access.js";
import { currentUser, requireAuth, type AuthUser } from "../auth.js";
import type { AppContext } from "../context.js";
import { nowIso, transaction, type DB } from "../db/index.js";
import { badRequest, forbidden, notFound, optionalText, parse, parseId, phone, text } from "../http.js";

export const ORDER_STATUSES = ["pending", "confirmed", "delivering", "completed", "cancelled"] as const;
export type OrderStatus = (typeof ORDER_STATUSES)[number];

/** Chủ cửa hàng (hoặc BQL) điều phối đơn theo luồng: chờ xác nhận → đã xác nhận → đang giao → hoàn thành. */
const MERCHANT_TRANSITIONS: Partial<Record<OrderStatus, OrderStatus[]>> = {
  pending: ["confirmed", "cancelled"],
  confirmed: ["delivering", "cancelled"],
  delivering: ["completed"],
};
/** Cư dân chỉ được huỷ khi cửa hàng chưa xác nhận. */
const RESIDENT_TRANSITIONS: Partial<Record<OrderStatus, OrderStatus[]>> = {
  pending: ["cancelled"],
};

const createSchema = z.object({
  shopId: z.number({ error: "Thiếu cửa hàng" }).int().positive(),
  items: z
    .array(
      z.object({
        productId: z.number().int().positive(),
        quantity: z.number().int().min(1, "Số lượng tối thiểu là 1").max(99, "Số lượng tối đa là 99"),
      }),
      { error: "Giỏ hàng trống" },
    )
    .min(1, "Giỏ hàng trống")
    .max(50, "Tối đa 50 món mỗi đơn"),
  deliveryApartment: text("Căn hộ nhận hàng", 30).optional(),
  contactPhone: phone.optional(),
  note: optionalText(500).default(""),
  paymentMethod: z.enum(["cod", "transfer"]).default("cod"),
});

const listSchema = z.object({
  as: z.enum(["resident", "merchant"]).default("resident"),
  status: z.enum(ORDER_STATUSES).optional(),
});

const statusSchema = z.object({ status: z.enum(ORDER_STATUSES, { error: "Trạng thái không hợp lệ" }) });

const ORDER_SELECT = `
  SELECT o.id, o.shop_id AS shopId, s.name AS shopName, s.phone AS shopPhone, s.owner_id AS shopOwnerId,
         o.resident_id AS residentId, u.full_name AS residentName, o.status,
         o.delivery_apartment AS deliveryApartment, o.contact_phone AS contactPhone, o.note,
         o.payment_method AS paymentMethod, o.total, o.created_at AS createdAt, o.updated_at AS updatedAt,
         rv.id AS reviewId, rv.rating AS reviewRating
  FROM orders o
  JOIN shops s ON s.id = o.shop_id
  JOIN users u ON u.id = o.resident_id
  LEFT JOIN reviews rv ON rv.order_id = o.id`;

interface OrderRow {
  id: number;
  shopId: number;
  shopOwnerId: number;
  residentId: number;
  status: OrderStatus;
  reviewId: number | null;
  reviewRating: number | null;
  [key: string]: unknown;
}

interface OrderItem {
  orderId: number;
  productId: number | null;
  productName: string;
  unitPrice: number;
  quantity: number;
}

function isShopSide(user: AuthUser, row: OrderRow) {
  return user.role === "admin" || user.id === row.shopOwnerId;
}

/** Các trạng thái mà `user` được phép chuyển đơn hàng sang. */
function allowedStatuses(user: AuthUser, row: OrderRow): OrderStatus[] {
  return [
    ...new Set([
      ...(isShopSide(user, row) ? (MERCHANT_TRANSITIONS[row.status] ?? []) : []),
      ...(row.residentId === user.id ? (RESIDENT_TRANSITIONS[row.status] ?? []) : []),
    ]),
  ];
}

/** Gắn danh sách món và các quyền thao tác của người xem vào đơn hàng. */
function present(db: DB, user: AuthUser, rows: OrderRow[]) {
  if (rows.length === 0) return [];
  const ids = rows.map((r) => r.id);
  const items = db
    .prepare(
      `SELECT order_id AS orderId, product_id AS productId, product_name AS productName,
              unit_price AS unitPrice, quantity
       FROM order_items WHERE order_id IN (${ids.map(() => "?").join(",")}) ORDER BY id`,
    )
    .all(...ids) as unknown as OrderItem[];
  return rows.map((row) => {
    const { shopOwnerId: _owner, reviewId, reviewRating, ...rest } = row;
    return {
      ...rest,
      review: reviewId ? { id: reviewId, rating: reviewRating } : null,
      items: items
        .filter((i) => i.orderId === row.id)
        .map(({ orderId: _orderId, ...item }) => item),
      viewerIsShop: isShopSide(user, row),
      allowedStatuses: allowedStatuses(user, row),
      canReview: row.residentId === user.id && row.status === "completed" && !reviewId,
    };
  });
}

function loadOrderRow(db: DB, id: number): OrderRow {
  const row = db.prepare(`${ORDER_SELECT} WHERE o.id = ?`).get(id) as OrderRow | undefined;
  if (!row) throw notFound("đơn hàng");
  return row;
}

export function loadOrderFor(db: DB, user: AuthUser, id: number) {
  const row = loadOrderRow(db, id);
  if (row.residentId !== user.id && !isShopSide(user, row)) throw notFound("đơn hàng");
  return { row, order: present(db, user, [row])[0] };
}

export function ordersRouter({ db }: AppContext): Router {
  const router = Router();
  router.use(requireAuth);

  router.post("/", (req, res) => {
    const user = currentUser(req);
    const input = parse(createSchema, req.body);

    const deliveryApartment = input.deliveryApartment ?? user.apartment;
    if (!deliveryApartment) throw badRequest("Vui lòng nhập căn hộ nhận hàng");

    const orderId = transaction(db, () => {
      const shop = getShopRecord(db, input.shopId);
      if (!shop.is_active) throw badRequest("Cửa hàng đang tạm ngưng nhận đơn");

      // Gộp các dòng trùng sản phẩm.
      const quantities = new Map<number, number>();
      for (const item of input.items) {
        quantities.set(item.productId, (quantities.get(item.productId) ?? 0) + item.quantity);
      }
      const productIds = [...quantities.keys()];
      const products = db
        .prepare(
          `SELECT id, name, price, is_available FROM products
           WHERE shop_id = ? AND id IN (${productIds.map(() => "?").join(",")})`,
        )
        .all(shop.id, ...productIds) as { id: number; name: string; price: number; is_available: number }[];

      const lines = productIds.map((productId) => {
        const product = products.find((p) => p.id === productId);
        if (!product) throw badRequest("Có sản phẩm không thuộc cửa hàng này");
        if (!product.is_available) throw badRequest(`"${product.name}" hiện đã hết hàng`);
        return { product, quantity: quantities.get(productId)! };
      });
      const total = lines.reduce((sum, l) => sum + l.product.price * l.quantity, 0);

      const { lastInsertRowid } = db
        .prepare(
          `INSERT INTO orders (resident_id, shop_id, delivery_apartment, contact_phone, note, payment_method, total)
           VALUES (?, ?, ?, ?, ?, ?, ?)`,
        )
        .run(
          user.id,
          shop.id,
          deliveryApartment,
          input.contactPhone ?? user.phone,
          input.note,
          input.paymentMethod,
          total,
        );
      const insertItem = db.prepare(
        "INSERT INTO order_items (order_id, product_id, product_name, unit_price, quantity) VALUES (?, ?, ?, ?, ?)",
      );
      for (const { product, quantity } of lines) {
        insertItem.run(lastInsertRowid, product.id, product.name, product.price, quantity);
      }
      return Number(lastInsertRowid);
    });

    res.status(201).json(loadOrderFor(db, user, orderId).order);
  });

  router.get("/", (req, res) => {
    const user = currentUser(req);
    const { as, status } = parse(listSchema, req.query);
    let where: string;
    const params: (string | number | null)[] = [];
    if (as === "merchant") {
      if (user.role === "resident") throw forbidden();
      where = user.role === "admin" ? "1 = 1" : "s.owner_id = ?";
      if (user.role !== "admin") params.push(user.id);
    } else {
      where = "o.resident_id = ?";
      params.push(user.id);
    }
    params.push(status ?? null, status ?? null);
    const rows = db
      .prepare(
        `${ORDER_SELECT} WHERE ${where} AND (? IS NULL OR o.status = ?) ORDER BY o.created_at DESC, o.id DESC LIMIT 200`,
      )
      .all(...params) as OrderRow[];
    res.json(present(db, user, rows));
  });

  router.get("/:id", (req, res) => {
    res.json(loadOrderFor(db, currentUser(req), parseId(req.params.id, "đơn hàng")).order);
  });

  router.patch("/:id/status", (req, res) => {
    const user = currentUser(req);
    const id = parseId(req.params.id, "đơn hàng");
    const { status } = parse(statusSchema, req.body);
    const { row } = loadOrderFor(db, user, id);

    if (!allowedStatuses(user, row).includes(status)) {
      throw badRequest("Không thể chuyển đơn hàng sang trạng thái này");
    }
    // Điều kiện status cũ trong WHERE chống hai người cập nhật cùng lúc.
    const { changes } = db
      .prepare("UPDATE orders SET status = ?, updated_at = ? WHERE id = ? AND status = ?")
      .run(status, nowIso(), id, row.status);
    if (!changes) throw badRequest("Đơn hàng vừa được cập nhật, vui lòng tải lại");
    res.json(loadOrderFor(db, user, id).order);
  });

  return router;
}
