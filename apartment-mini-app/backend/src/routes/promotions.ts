import { Router } from "express";
import { z } from "zod";
import { canManageShop, getShopRecord, requireShopManager } from "../access.js";
import { currentUser, requireAuth, requireRole } from "../auth.js";
import type { AppContext } from "../context.js";
import { nowIso, type DB } from "../db/index.js";
import { badRequest, forbidden, imageUrl, isoDate, notFound, optionalText, parse, parseId, text } from "../http.js";

export const PROMOTION_STATUSES = ["pending", "approved", "rejected"] as const;

const createSchema = z
  .object({
    shopId: z.number().int().positive().nullable().optional(),
    title: text("Tiêu đề", 150),
    content: optionalText(2000).default(""),
    discountLabel: optionalText(50).default(""),
    imageUrl,
    startsAt: isoDate("Ngày bắt đầu"),
    endsAt: isoDate("Ngày kết thúc"),
  })
  .refine((p) => p.endsAt > p.startsAt, {
    message: "Ngày kết thúc phải sau ngày bắt đầu",
    path: ["endsAt"],
  });

const reviewSchema = z.object({
  status: z.enum(["approved", "rejected"], { error: "Trạng thái không hợp lệ" }),
});

const SELECT = `
  SELECT p.id, p.shop_id AS shopId, s.name AS shopName, s.category AS shopCategory,
         p.title, p.content, p.discount_label AS discountLabel, p.image_url AS imageUrl,
         p.starts_at AS startsAt, p.ends_at AS endsAt, p.status, p.created_at AS createdAt,
         u.full_name AS createdByName
  FROM promotions p
  LEFT JOIN shops s ON s.id = p.shop_id
  LEFT JOIN users u ON u.id = p.created_by`;

function loadPromotion(db: DB, id: number) {
  const row = db.prepare(`${SELECT} WHERE p.id = ?`).get(id) as
    | ({ shopId: number | null } & Record<string, unknown>)
    | undefined;
  if (!row) throw notFound("tin quảng bá");
  return row;
}

export function promotionsRouter({ db }: AppContext): Router {
  const router = Router();

  /** Bảng tin công khai: chỉ tin đã duyệt, còn hiệu lực, và cửa hàng đang hoạt động. */
  router.get("/", (_req, res) => {
    const now = nowIso();
    const rows = db
      .prepare(
        `${SELECT} WHERE p.status = 'approved' AND p.starts_at <= ? AND p.ends_at > ?
           AND (p.shop_id IS NULL OR s.is_active = 1)
         ORDER BY p.starts_at DESC LIMIT 50`,
      )
      .all(now, now);
    res.json(rows);
  });

  /** BQL xem tất cả để duyệt; chủ cửa hàng xem tin của cửa hàng mình. */
  router.get("/manage", requireRole("merchant", "admin"), (req, res) => {
    const user = currentUser(req);
    const rows =
      user.role === "admin"
        ? db
            .prepare(`${SELECT} ORDER BY CASE p.status WHEN 'pending' THEN 0 ELSE 1 END, p.created_at DESC`)
            .all()
        : db.prepare(`${SELECT} WHERE s.owner_id = ? ORDER BY p.created_at DESC`).all(user.id);
    res.json(rows);
  });

  router.post("/", requireRole("merchant", "admin"), (req, res) => {
    const user = currentUser(req);
    const input = parse(createSchema, req.body);
    if (input.shopId) requireShopManager(db, user, input.shopId);
    else if (user.role !== "admin") throw badRequest("Vui lòng chọn cửa hàng cần quảng bá");

    // Tin của BQL được đăng ngay; tin của cửa hàng chờ BQL duyệt.
    const status = user.role === "admin" ? "approved" : "pending";
    const { lastInsertRowid } = db
      .prepare(
        `INSERT INTO promotions (shop_id, created_by, title, content, discount_label, image_url, starts_at, ends_at, status)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        input.shopId ?? null,
        user.id,
        input.title,
        input.content,
        input.discountLabel,
        input.imageUrl ?? null,
        input.startsAt,
        input.endsAt,
        status,
      );
    res.status(201).json(loadPromotion(db, Number(lastInsertRowid)));
  });

  router.patch("/:id/status", requireRole("admin"), (req, res) => {
    const id = parseId(req.params.id, "tin quảng bá");
    loadPromotion(db, id);
    const { status } = parse(reviewSchema, req.body);
    db.prepare("UPDATE promotions SET status = ? WHERE id = ?").run(status, id);
    res.json(loadPromotion(db, id));
  });

  router.delete("/:id", requireAuth, (req, res) => {
    const id = parseId(req.params.id, "tin quảng bá");
    const promo = loadPromotion(db, id);
    const user = currentUser(req);
    const allowed =
      user.role === "admin" || (promo.shopId !== null && canManageShop(user, getShopRecord(db, promo.shopId)));
    if (!allowed) throw forbidden();
    db.prepare("DELETE FROM promotions WHERE id = ?").run(id);
    res.status(204).end();
  });

  return router;
}
