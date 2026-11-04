import { loadConfig } from "../config.js";
import { openDatabase } from "./index.js";
import { seedDemoData } from "./seed.js";

try {
  process.loadEnvFile();
} catch {
  // Không có file .env.
}

const config = loadConfig();
const db = openDatabase(config.databasePath);
console.log(
  seedDemoData(db)
    ? `Đã nạp dữ liệu mẫu vào ${config.databasePath}`
    : "Database đã có dữ liệu, bỏ qua. Xoá file database nếu muốn nạp lại.",
);
db.close();
