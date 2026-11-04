import { Router } from "express";
import { z } from "zod";
import {
  currentUser,
  findUser,
  hashPassword,
  requireAuth,
  signToken,
  USER_COLUMNS,
  verifyPassword,
  type AuthUser,
} from "../auth.js";
import type { AppContext } from "../context.js";
import { conflict, parse, phone, text, unauthorized } from "../http.js";

const password = z
  .string({ error: "Mật khẩu là bắt buộc" })
  .min(6, "Mật khẩu tối thiểu 6 ký tự")
  .max(100, "Mật khẩu quá dài");

const apartment = z.string().trim().max(30, "Mã căn hộ tối đa 30 ký tự");

const registerSchema = z.object({
  phone,
  password,
  fullName: text("Họ tên", 100),
  apartment: apartment.optional(),
  // Không cho tự đăng ký tài khoản BQL (admin).
  role: z.enum(["resident", "merchant"]).default("resident"),
});

const loginSchema = z.object({ phone, password: z.string().min(1, "Vui lòng nhập mật khẩu") });

const updateMeSchema = z.object({
  fullName: text("Họ tên", 100).optional(),
  apartment: apartment.nullable().optional(),
});

export function authRouter({ db, config }: AppContext): Router {
  const router = Router();
  const session = (user: AuthUser) => ({ token: signToken(user, config.jwtSecret), user });

  router.post("/register", (req, res) => {
    const input = parse(registerSchema, req.body);
    const exists = db.prepare("SELECT 1 FROM users WHERE phone = ?").get(input.phone);
    if (exists) throw conflict("Số điện thoại đã được đăng ký");

    const { lastInsertRowid } = db
      .prepare(
        "INSERT INTO users (phone, password_hash, full_name, role, apartment) VALUES (?, ?, ?, ?, ?)",
      )
      .run(input.phone, hashPassword(input.password), input.fullName, input.role, input.apartment || null);
    res.status(201).json(session(findUser(db, Number(lastInsertRowid))!));
  });

  router.post("/login", (req, res) => {
    const input = parse(loginSchema, req.body);
    const row = db
      .prepare(`SELECT ${USER_COLUMNS}, password_hash FROM users WHERE phone = ?`)
      .get(input.phone) as (AuthUser & { password_hash: string }) | undefined;
    if (!row || !verifyPassword(input.password, row.password_hash)) {
      throw unauthorized("Số điện thoại hoặc mật khẩu không đúng");
    }
    const { password_hash: _hash, ...user } = row;
    res.json(session(user));
  });

  router.get("/me", requireAuth, (req, res) => {
    res.json(currentUser(req));
  });

  router.patch("/me", requireAuth, (req, res) => {
    const user = currentUser(req);
    const input = parse(updateMeSchema, req.body);
    db.prepare(
      "UPDATE users SET full_name = COALESCE(?, full_name), apartment = CASE WHEN ? THEN ? ELSE apartment END WHERE id = ?",
    ).run(
      input.fullName ?? null,
      input.apartment !== undefined ? 1 : 0,
      input.apartment || null,
      user.id,
    );
    res.json(findUser(db, user.id));
  });

  return router;
}
