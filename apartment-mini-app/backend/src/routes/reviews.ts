import { Router } from "express";
import { z } from "zod";
import { canManageShop, getShopRecord } from "../access.js";
import { currentUser, requireAuth } from "../auth.js";
import type { AppContext } from "../context.js";
import { nowIso, type DB } from "../db/index.js";
import { badRequest, conflict, forbidden, notFound, optionalText, parse, parseId, text } from "../http.js";
import { loadOrderFor } from "./orders.js";

const createSchema = z.object({
  rating: z
    .number({ error: "Vui lòng chọn số sao" })
    .int()
    .min(1, "Đánh giá từ 1 đến 5 sao")
    .max(5, "Đánh giá từ 1 đến 5 sao"),
  comment: optionalText(1000).default(""),
});
const replySchema = z.object({ reply: text("Nội dung phản hồi", 1000) });
const listSchema = z.object({
  limit: z.coerce.number().int().min(1).max(100).default(20),
  offset: z.coerce.number().int().min(0).default(0),
});

const REVIEW_SELECT = `
  SELECT r.id, r.shop_id AS shopId, r.order_id AS orderId, r.rating, r.comment, r.reply,
         r.replied_at AS repliedAt, r.created_at AS createdAt,
         u.full_name AS residentName
  FROM reviews r JOIN users u ON u.id = r.resident_id`;

function loadReview(db: DB, id: number) {
  const row = db.prepare(`${REVIEW_SELECT} WHERE r.id = ?`).get(id) as
    | ({ shopId: number } & Record<string, unknown>)
    | undefined;
  if (!row) throw notFound("đánh giá");
  return row;
}

/**
 * Mount tại `/api`:
 *  - GET  /shops/:id/reviews   danh sách đánh giá + thống kê sao
 *  - POST /orders/:id/review   cư dân đánh giá đơn đã hoàn thành (mỗi đơn 1 lần)
 *  - POST /reviews/:id/reply   chủ cửa hàng phản hồi
 */
export function reviewsRouter({ db }: AppContext): Router {
  const router = Router();

  router.get("/shops/:id/reviews", (req, res) => {
    const shopId = parseId(req.params.id, "cửa hàng");
    getShopRecord(db, shopId);
    const { limit, offset } = parse(listSchema, req.query);
    const items = db
      .prepare(`${REVIEW_SELECT} WHERE r.shop_id = ? ORDER BY r.created_at DESC, r.id DESC LIMIT ? OFFSET ?`)
      .all(shopId, limit, offset);
    const stats = db
      .prepare("SELECT rating, COUNT(*) AS count FROM reviews WHERE shop_id = ? GROUP BY rating")
      .all(shopId) as { rating: number; count: number }[];
    const distribution = [1, 2, 3, 4, 5].map((star) => ({
      rating: star,
      count: stats.find((s) => s.rating === star)?.count ?? 0,
    }));
    const total = distribution.reduce((sum, d) => sum + d.count, 0);
    const average = total
      ? Math.round((distribution.reduce((sum, d) => sum + d.rating * d.count, 0) / total) * 10) / 10
      : 0;
    res.json({ items, total, average, distribution });
  });

  router.post("/orders/:id/review", requireAuth, (req, res) => {
    const user = currentUser(req);
    const orderId = parseId(req.params.id, "đơn hàng");
    const { row } = loadOrderFor(db, user, orderId);
    if (row.residentId !== user.id) throw forbidden("Chỉ người đặt hàng mới được đánh giá");
    if (row.status !== "completed") throw badRequest("Chỉ đánh giá được đơn hàng đã hoàn thành");
    if (row.reviewId) throw conflict("Bạn đã đánh giá đơn hàng này");

    const input = parse(createSchema, req.body);
    const { lastInsertRowid } = db
      .prepare(
        "INSERT INTO reviews (shop_id, order_id, resident_id, rating, comment) VALUES (?, ?, ?, ?, ?)",
      )
      .run(row.shopId, orderId, user.id, input.rating, input.comment);
    res.status(201).json(loadReview(db, Number(lastInsertRowid)));
  });

  router.post("/reviews/:id/reply", requireAuth, (req, res) => {
    const id = parseId(req.params.id, "đánh giá");
    const review = loadReview(db, id);
    if (!canManageShop(currentUser(req), getShopRecord(db, review.shopId))) {
      throw forbidden("Chỉ chủ cửa hàng mới được phản hồi đánh giá");
    }
    const { reply } = parse(replySchema, req.body);
    db.prepare("UPDATE reviews SET reply = ?, replied_at = ? WHERE id = ?").run(reply, nowIso(), id);
    res.json(loadReview(db, id));
  });

  return router;
}
