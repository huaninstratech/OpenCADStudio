import { existsSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export interface Config {
  port: number;
  databasePath: string;
  jwtSecret: string;
  /** Nạp dữ liệu mẫu khi database còn trống. */
  seedDemo: boolean;
  /** Thư mục build của frontend; backend sẽ phục vụ luôn SPA nếu thư mục tồn tại. */
  staticDir: string | null;
  /** Danh sách origin được phép gọi API (vd. khi frontend chạy trên Zalo Mini App). */
  corsOrigins: string[];
  /** Tạo tài khoản BQL đầu tiên khi chưa có admin nào. */
  bootstrapAdmin: { phone: string; password: string } | null;
}

const DEV_SECRET = "dev-only-secret-do-not-use-in-production";

function bool(value: string | undefined, fallback: boolean): boolean {
  if (value === undefined || value === "") return fallback;
  return ["1", "true", "yes", "on"].includes(value.toLowerCase());
}

export function loadConfig(env: NodeJS.ProcessEnv = process.env): Config {
  const production = env.NODE_ENV === "production";
  const jwtSecret = env.JWT_SECRET ?? (production ? "" : DEV_SECRET);
  if (!jwtSecret) {
    throw new Error("JWT_SECRET phải được đặt khi chạy NODE_ENV=production");
  }

  const here = dirname(fileURLToPath(import.meta.url));
  const defaultStatic = resolve(here, "../../frontend/dist");
  const staticDir = env.STATIC_DIR ? resolve(env.STATIC_DIR) : defaultStatic;

  return {
    port: Number(env.PORT ?? 4000),
    databasePath: env.DATABASE_PATH ?? "./data/cudan.db",
    jwtSecret,
    seedDemo: bool(env.SEED_DEMO, !production),
    staticDir: existsSync(staticDir) ? staticDir : null,
    corsOrigins: (env.CORS_ORIGIN ?? "")
      .split(",")
      .map((o) => o.trim())
      .filter(Boolean),
    bootstrapAdmin:
      env.ADMIN_PHONE && env.ADMIN_PASSWORD
        ? { phone: env.ADMIN_PHONE, password: env.ADMIN_PASSWORD }
        : null,
  };
}
