import { randomBytes, scryptSync, timingSafeEqual } from "node:crypto";
import type { RequestHandler } from "express";
import jwt from "jsonwebtoken";
import type { DB } from "./db/index.js";
import { forbidden, unauthorized } from "./http.js";

export const ROLES = ["resident", "merchant", "admin"] as const;
export type Role = (typeof ROLES)[number];

export interface AuthUser {
  id: number;
  phone: string;
  fullName: string;
  role: Role;
  apartment: string | null;
  createdAt: string;
}

declare global {
  namespace Express {
    interface Request {
      user?: AuthUser;
    }
  }
}

const KEY_LENGTH = 64;
const TOKEN_TTL = "30d";

export function hashPassword(password: string): string {
  const salt = randomBytes(16);
  const hash = scryptSync(password, salt, KEY_LENGTH);
  return ["scrypt", salt.toString("base64"), hash.toString("base64")].join("$");
}

export function verifyPassword(password: string, stored: string): boolean {
  const [scheme, saltB64, hashB64] = stored.split("$");
  if (scheme !== "scrypt" || !saltB64 || !hashB64) return false;
  const expected = Buffer.from(hashB64, "base64");
  const actual = scryptSync(password, Buffer.from(saltB64, "base64"), expected.length);
  return timingSafeEqual(actual, expected);
}

export function signToken(user: Pick<AuthUser, "id" | "role">, secret: string): string {
  return jwt.sign({ role: user.role }, secret, { subject: String(user.id), expiresIn: TOKEN_TTL });
}

export const USER_COLUMNS =
  "id, phone, full_name AS fullName, role, apartment, created_at AS createdAt";

export function findUser(db: DB, id: number): AuthUser | undefined {
  return db.prepare(`SELECT ${USER_COLUMNS} FROM users WHERE id = ?`).get(id) as
    | AuthUser
    | undefined;
}

/**
 * Đọc header `Authorization: Bearer <token>` (nếu có) và gắn `req.user`.
 * Người dùng được nạp lại từ DB ở mỗi request để thay đổi quyền có hiệu lực ngay.
 */
export function authenticate(db: DB, secret: string): RequestHandler {
  return (req, _res, next) => {
    const header = req.headers.authorization;
    if (!header?.startsWith("Bearer ")) return next();
    let subject: number;
    try {
      const payload = jwt.verify(header.slice(7), secret) as jwt.JwtPayload;
      subject = Number(payload.sub);
    } catch {
      return next(unauthorized("Phiên đăng nhập đã hết hạn, vui lòng đăng nhập lại"));
    }
    const user = findUser(db, subject);
    if (!user) return next(unauthorized("Phiên đăng nhập không còn hợp lệ"));
    req.user = user;
    next();
  };
}

export function currentUser(req: { user?: AuthUser }): AuthUser {
  if (!req.user) throw unauthorized();
  return req.user;
}

export const requireAuth: RequestHandler = (req, _res, next) => {
  currentUser(req);
  next();
};

export function requireRole(...roles: Role[]): RequestHandler {
  return (req, _res, next) => {
    const user = currentUser(req);
    if (!roles.includes(user.role)) throw forbidden();
    next();
  };
}
