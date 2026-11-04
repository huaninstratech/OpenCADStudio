import { Router } from "express";
import { z } from "zod";
import { requireRole } from "../auth.js";
import type { AppContext } from "../context.js";
import { transaction } from "../db/index.js";
import { optionalText, parse, text } from "../http.js";

const DEFAULT_INFO = {
  name: "Chung cư của tôi",
  address: "",
  description: "",
  amenities: [] as string[],
};

const updateSchema = z.object({
  name: text("Tên chung cư", 150),
  address: text("Địa chỉ", 300),
  description: optionalText(2000).default(""),
  amenities: z.array(z.string().trim().min(1).max(100)).max(50).default([]),
  contacts: z
    .array(
      z.object({
        name: text("Tên liên hệ", 100),
        phone: text("Số điện thoại", 20),
        description: optionalText(200).default(""),
      }),
    )
    .max(30)
    .default([]),
});

export function buildingRouter({ db }: AppContext): Router {
  const router = Router();

  const load = () => {
    const row = db
      .prepare("SELECT name, address, description, amenities FROM building_info WHERE id = 1")
      .get() as { name: string; address: string; description: string; amenities: string } | undefined;
    const contacts = db
      .prepare("SELECT id, name, phone, description FROM contacts ORDER BY sort_order, id")
      .all();
    const info = row ? { ...row, amenities: JSON.parse(row.amenities) as string[] } : DEFAULT_INFO;
    return { ...info, contacts };
  };

  router.get("/", (_req, res) => {
    res.json(load());
  });

  router.put("/", requireRole("admin"), (req, res) => {
    const input = parse(updateSchema, req.body);
    transaction(db, () => {
      db.prepare(
        `INSERT INTO building_info (id, name, address, description, amenities) VALUES (1, ?, ?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET name = excluded.name, address = excluded.address,
           description = excluded.description, amenities = excluded.amenities`,
      ).run(input.name, input.address, input.description, JSON.stringify(input.amenities));
      db.exec("DELETE FROM contacts");
      const insert = db.prepare(
        "INSERT INTO contacts (name, phone, description, sort_order) VALUES (?, ?, ?, ?)",
      );
      input.contacts.forEach((c, i) => insert.run(c.name, c.phone, c.description, i));
    });
    res.json(load());
  });

  return router;
}
