import { Router } from "express";
import { z } from "zod";
import { currentUser, requireRole } from "../auth.js";
import type { AppContext } from "../context.js";
import { nowIso } from "../db/index.js";
import { applyPatch, notFound, parse, parseId, text } from "../http.js";

export const ANNOUNCEMENT_CATEGORIES = ["general", "maintenance", "event", "fee", "security"] as const;

const fields = {
  title: text("Tiêu đề", 200),
  content: text("Nội dung", 10_000),
  category: z.enum(ANNOUNCEMENT_CATEGORIES),
  pinned: z.boolean(),
};
const createSchema = z.object({
  ...fields,
  category: fields.category.default("general"),
  pinned: fields.pinned.default(false),
});
const updateSchema = z.object(fields).partial();
const listSchema = z.object({
  category: z.enum(ANNOUNCEMENT_CATEGORIES).optional(),
  limit: z.coerce.number().int().min(1).max(100).default(50),
});

const SELECT = `
  SELECT a.id, a.title, a.content, a.category, a.pinned, a.created_at AS createdAt,
         a.updated_at AS updatedAt, u.full_name AS authorName
  FROM announcements a LEFT JOIN users u ON u.id = a.author_id`;

type Row = { pinned: number } & Record<string, unknown>;
const toAnnouncement = (row: Row) => ({ ...row, pinned: row.pinned === 1 });

export function announcementsRouter({ db }: AppContext): Router {
  const router = Router();

  const find = (id: number) => {
    const row = db.prepare(`${SELECT} WHERE a.id = ?`).get(id) as Row | undefined;
    if (!row) throw notFound("thông báo");
    return toAnnouncement(row);
  };

  router.get("/", (req, res) => {
    const { category, limit } = parse(listSchema, req.query);
    const rows = db
      .prepare(
        `${SELECT} WHERE (? IS NULL OR a.category = ?) ORDER BY a.pinned DESC, a.created_at DESC LIMIT ?`,
      )
      .all(category ?? null, category ?? null, limit) as Row[];
    res.json(rows.map(toAnnouncement));
  });

  router.get("/:id", (req, res) => {
    res.json(find(parseId(req.params.id, "thông báo")));
  });

  router.post("/", requireRole("admin"), (req, res) => {
    const input = parse(createSchema, req.body);
    const { lastInsertRowid } = db
      .prepare(
        "INSERT INTO announcements (title, content, category, pinned, author_id) VALUES (?, ?, ?, ?, ?)",
      )
      .run(input.title, input.content, input.category, input.pinned ? 1 : 0, currentUser(req).id);
    res.status(201).json(find(Number(lastInsertRowid)));
  });

  router.patch("/:id", requireRole("admin"), (req, res) => {
    const id = parseId(req.params.id, "thông báo");
    const current = find(id) as unknown as z.infer<typeof createSchema>;
    const input = applyPatch(current, parse(updateSchema, req.body));
    db.prepare(
      "UPDATE announcements SET title = ?, content = ?, category = ?, pinned = ?, updated_at = ? WHERE id = ?",
    ).run(input.title, input.content, input.category, input.pinned ? 1 : 0, nowIso(), id);
    res.json(find(id));
  });

  router.delete("/:id", requireRole("admin"), (req, res) => {
    const id = parseId(req.params.id, "thông báo");
    const { changes } = db.prepare("DELETE FROM announcements WHERE id = ?").run(id);
    if (!changes) throw notFound("thông báo");
    res.status(204).end();
  });

  return router;
}
