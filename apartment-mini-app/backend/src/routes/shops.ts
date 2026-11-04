import { Router, type Request } from "express";
import { z } from "zod";
import { canManageShop, foldVietnamese, getShopRecord, requireShopManager } from "../access.js";
import { currentUser, requireRole } from "../auth.js";
import type { AppContext } from "../context.js";
import type { DB } from "../db/index.js";
import { applyPatch, forbidden, imageUrl, notFound, optionalText, parse, parseId, text } from "../http.js";

export const SHOP_CATEGORIES = ["food", "drink", "grocery", "service", "other"] as const;

const shopFields = {
  name: text("Tên cửa hàng", 100),
  description: optionalText(1000),
  category: z.enum(SHOP_CATEGORIES, { error: "Danh mục không hợp lệ" }),
  phone: optionalText(20),
  location: optionalText(150),
  openingHours: optionalText(100),
  imageUrl,
  isActive: z.boolean(),
};
const createShopSchema = z.object({
  ...shopFields,
  description: shopFields.description.default(""),
  phone: shopFields.phone.default(""),
  location: shopFields.location.default(""),
  openingHours: shopFields.openingHours.default(""),
  isActive: shopFields.isActive.default(true),
});
const updateShopSchema = z.object(shopFields).partial();

const productFields = {
  name: text("Tên sản phẩm", 120),
  description: optionalText(1000),
  price: z
    .number({ error: "Giá phải là số" })
    .int("Giá phải là số nguyên (VNĐ)")
    .min(0, "Giá không được âm")
    .max(100_000_000, "Giá quá lớn"),
  imageUrl,
  isAvailable: z.boolean(),
};
const createProductSchema = z.object({
  ...productFields,
  description: productFields.description.default(""),
  isAvailable: productFields.isAvailable.default(true),
});
const updateProductSchema = z.object(productFields).partial();

const listSchema = z.object({
  category: z.enum(SHOP_CATEGORIES).optional(),
  q: z.string().trim().max(100).optional(),
});

const SHOP_SELECT = `
  SELECT s.id, s.owner_id AS ownerId, s.name, s.description, s.category, s.phone, s.location,
         s.opening_hours AS openingHours, s.image_url AS imageUrl, s.is_active AS isActive,
         s.created_at AS createdAt,
         COALESCE(r.rating_avg, 0) AS ratingAvg, COALESCE(r.review_count, 0) AS reviewCount
  FROM shops s
  LEFT JOIN (
    SELECT shop_id, ROUND(AVG(rating), 1) AS rating_avg, COUNT(*) AS review_count
    FROM reviews GROUP BY shop_id
  ) r ON r.shop_id = s.id`;

const PRODUCT_SELECT = `
  SELECT id, shop_id AS shopId, name, description, price, image_url AS imageUrl,
         is_available AS isAvailable
  FROM products`;

type ShopRow = { isActive: number; name: string; description: string } & Record<string, unknown>;
type ProductRow = { id: number; shopId: number; isAvailable: number } & Record<string, unknown>;

const toShop = (row: ShopRow) => ({ ...row, isActive: row.isActive === 1 });
const toProduct = (row: ProductRow) => ({ ...row, isAvailable: row.isAvailable === 1 });

function loadShop(db: DB, id: number) {
  const row = db.prepare(`${SHOP_SELECT} WHERE s.id = ?`).get(id) as ShopRow | undefined;
  if (!row) throw notFound("cửa hàng");
  return toShop(row);
}

function loadProduct(db: DB, id: number) {
  const row = db.prepare(`${PRODUCT_SELECT} WHERE id = ?`).get(id) as ProductRow | undefined;
  if (!row) throw notFound("sản phẩm");
  return toProduct(row);
}

export function shopsRouter({ db }: AppContext): Router {
  const router = Router();

  router.get("/", (req, res) => {
    const { category, q } = parse(listSchema, req.query);
    let rows = db
      .prepare(
        `${SHOP_SELECT} WHERE s.is_active = 1 AND (? IS NULL OR s.category = ?)
         ORDER BY reviewCount DESC, s.name`,
      )
      .all(category ?? null, category ?? null) as ShopRow[];
    if (q) {
      const needle = foldVietnamese(q);
      rows = rows.filter((s) => foldVietnamese(`${s.name} ${s.description}`).includes(needle));
    }
    res.json(rows.map(toShop));
  });

  router.get("/mine", requireRole("merchant", "admin"), (req, res) => {
    const rows = db
      .prepare(`${SHOP_SELECT} WHERE s.owner_id = ? ORDER BY s.created_at`)
      .all(currentUser(req).id) as ShopRow[];
    res.json(rows.map(toShop));
  });

  router.get("/:id", (req, res) => {
    const id = parseId(req.params.id, "cửa hàng");
    const record = getShopRecord(db, id);
    const manage = canManageShop(req.user, record);
    if (!record.is_active && !manage) throw notFound("cửa hàng");

    const products = db
      .prepare(`${PRODUCT_SELECT} WHERE shop_id = ? AND (? OR is_available = 1) ORDER BY id`)
      .all(id, manage ? 1 : 0) as ProductRow[];
    res.json({ ...loadShop(db, id), products: products.map(toProduct), canManage: manage });
  });

  router.post("/", requireRole("merchant", "admin"), (req, res) => {
    const input = parse(createShopSchema, req.body);
    const { lastInsertRowid } = db
      .prepare(
        `INSERT INTO shops (owner_id, name, description, category, phone, location, opening_hours, image_url, is_active)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
      )
      .run(
        currentUser(req).id,
        input.name,
        input.description,
        input.category,
        input.phone,
        input.location,
        input.openingHours,
        input.imageUrl ?? null,
        input.isActive ? 1 : 0,
      );
    res.status(201).json(loadShop(db, Number(lastInsertRowid)));
  });

  router.patch("/:id", (req, res) => {
    const id = parseId(req.params.id, "cửa hàng");
    requireShopManager(db, currentUser(req), id);
    const next = applyPatch(
      loadShop(db, id) as unknown as z.infer<typeof createShopSchema>,
      parse(updateShopSchema, req.body),
    );
    db.prepare(
      `UPDATE shops SET name = ?, description = ?, category = ?, phone = ?, location = ?,
         opening_hours = ?, image_url = ?, is_active = ? WHERE id = ?`,
    ).run(
      next.name,
      next.description,
      next.category,
      next.phone,
      next.location,
      next.openingHours,
      next.imageUrl ?? null,
      next.isActive ? 1 : 0,
      id,
    );
    res.json(loadShop(db, id));
  });

  router.post("/:id/products", (req, res) => {
    const shopId = parseId(req.params.id, "cửa hàng");
    requireShopManager(db, currentUser(req), shopId);
    const input = parse(createProductSchema, req.body);
    const { lastInsertRowid } = db
      .prepare(
        "INSERT INTO products (shop_id, name, description, price, image_url, is_available) VALUES (?, ?, ?, ?, ?, ?)",
      )
      .run(shopId, input.name, input.description, input.price, input.imageUrl ?? null, input.isAvailable ? 1 : 0);
    res.status(201).json(loadProduct(db, Number(lastInsertRowid)));
  });

  return router;
}

export function productsRouter({ db }: AppContext): Router {
  const router = Router();

  const managedProduct = (req: Request) => {
    const product = loadProduct(db, parseId(req.params.id, "sản phẩm"));
    const user = currentUser(req);
    if (!canManageShop(user, getShopRecord(db, product.shopId))) {
      throw forbidden("Bạn không quản lý sản phẩm này");
    }
    return product;
  };

  router.patch("/:id", (req, res) => {
    const current = managedProduct(req);
    const next = applyPatch(
      current as unknown as z.infer<typeof createProductSchema>,
      parse(updateProductSchema, req.body),
    );
    db.prepare(
      "UPDATE products SET name = ?, description = ?, price = ?, image_url = ?, is_available = ? WHERE id = ?",
    ).run(next.name, next.description, next.price, next.imageUrl ?? null, next.isAvailable ? 1 : 0, current.id);
    res.json(loadProduct(db, current.id));
  });

  router.delete("/:id", (req, res) => {
    const product = managedProduct(req);
    // order_items giữ lại tên & giá tại thời điểm đặt, product_id sẽ thành NULL.
    db.prepare("DELETE FROM products WHERE id = ?").run(product.id);
    res.status(204).end();
  });

  return router;
}
