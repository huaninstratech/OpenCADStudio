import type { ErrorRequestHandler, RequestHandler } from "express";
import { z, type ZodType } from "zod";

export class HttpError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly details?: unknown,
  ) {
    super(message);
  }
}

export const badRequest = (message: string, details?: unknown) =>
  new HttpError(400, message, details);
export const unauthorized = (message = "Vui lòng đăng nhập") => new HttpError(401, message);
export const forbidden = (message = "Bạn không có quyền thực hiện thao tác này") =>
  new HttpError(403, message);
export const notFound = (what: string) => new HttpError(404, `Không tìm thấy ${what}`);
export const conflict = (message: string) => new HttpError(409, message);

/** Kiểm tra dữ liệu đầu vào bằng zod; ném lỗi 400 kèm danh sách trường sai. */
export function parse<T>(schema: ZodType<T>, data: unknown): T {
  const result = schema.safeParse(data);
  if (!result.success) {
    throw badRequest(
      result.error.issues[0]?.message ?? "Dữ liệu không hợp lệ",
      result.error.issues.map((i) => ({ field: i.path.join("."), message: i.message })),
    );
  }
  return result.data;
}

export function parseId(value: unknown, what = "bản ghi"): number {
  const id = Number(value);
  if (!Number.isSafeInteger(id) || id <= 0) throw notFound(what);
  return id;
}

/* ---------- Các schema dùng chung ---------- */

export const text = (label: string, max: number) =>
  z
    .string({ error: `${label} là bắt buộc` })
    .trim()
    .min(1, `${label} không được để trống`)
    .max(max, `${label} tối đa ${max} ký tự`);

/**
 * Chuỗi tuỳ chọn. Cố ý KHÔNG gắn `.default()`: zod 4 áp default cả bên trong
 * `.partial()`, nên schema PATCH sẽ ghi đè dữ liệu cũ bằng giá trị mặc định.
 * Schema tạo mới tự thêm `.default("")` khi cần.
 */
export const optionalText = (max: number) =>
  z.string().trim().max(max, `Tối đa ${max} ký tự`);

/** Gộp bản vá vào bản ghi hiện tại, bỏ qua các khoá có giá trị `undefined`. */
export function applyPatch<T extends object>(current: T, patch: Partial<T>): T {
  const next = { ...current };
  for (const [key, value] of Object.entries(patch)) {
    if (value !== undefined) (next as Record<string, unknown>)[key] = value;
  }
  return next;
}

export const phone = z
  .string({ error: "Số điện thoại là bắt buộc" })
  .trim()
  .regex(/^(\+?84|0)\d{9,10}$/, "Số điện thoại không hợp lệ");

export const isoDate = (label: string) =>
  z
    .string({ error: `${label} là bắt buộc` })
    .refine((v) => !Number.isNaN(Date.parse(v)), `${label} không hợp lệ`)
    .transform((v) => new Date(v).toISOString());

export const imageUrl = z
  .url("Đường dẫn ảnh không hợp lệ")
  .max(500)
  .nullable()
  .optional();

/* ---------- Middleware ---------- */

export const notFoundApi: RequestHandler = (_req, res) => {
  res.status(404).json({ error: "Không tìm thấy API" });
};

export const errorHandler: ErrorRequestHandler = (err, _req, res, _next) => {
  if (err instanceof HttpError) {
    res.status(err.status).json({ error: err.message, details: err.details });
    return;
  }
  // Lỗi do express.json() ném ra khi body sai cú pháp hoặc quá lớn.
  if (err?.type === "entity.parse.failed") {
    res.status(400).json({ error: "Body JSON không hợp lệ" });
    return;
  }
  if (err?.type === "entity.too.large") {
    res.status(413).json({ error: "Dữ liệu gửi lên quá lớn" });
    return;
  }
  console.error(err);
  res.status(500).json({ error: "Lỗi máy chủ, vui lòng thử lại sau" });
};
