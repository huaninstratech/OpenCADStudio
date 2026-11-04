import { createApp } from "./app.js";
import { loadConfig } from "./config.js";
import { openDatabase } from "./db/index.js";
import { ensureAdmin, seedDemoData } from "./db/seed.js";

try {
  process.loadEnvFile();
} catch {
  // Không có file .env — dùng biến môi trường sẵn có.
}

const config = loadConfig();
const db = openDatabase(config.databasePath);

if (config.bootstrapAdmin) ensureAdmin(db, config.bootstrapAdmin);
if (config.seedDemo && seedDemoData(db)) {
  console.log("Đã nạp dữ liệu mẫu (xem README để biết tài khoản demo).");
}

const app = createApp(db, config);
const server = app.listen(config.port, () => {
  console.log(`CưDân Hub API đang chạy tại http://localhost:${config.port}`);
  if (config.staticDir) console.log(`Phục vụ giao diện từ ${config.staticDir}`);
});

for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    server.close(() => {
      db.close();
      process.exit(0);
    });
  });
}
