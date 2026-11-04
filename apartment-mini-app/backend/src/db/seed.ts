import { hashPassword } from "../auth.js";
import { transaction, type DB } from "./index.js";

const DAY = 24 * 60 * 60 * 1000;
const at = (offsetDays: number) => new Date(Date.now() + offsetDays * DAY).toISOString();

/** Tài khoản demo — chỉ dùng cho môi trường phát triển. */
export const DEMO_ACCOUNTS = {
  admin: { phone: "0900000001", password: "admin123", fullName: "Ban Quản Lý Hoa Sen" },
  merchantLan: { phone: "0900000002", password: "shop123", fullName: "Nguyễn Thị Lan" },
  merchantMay: { phone: "0900000003", password: "shop123", fullName: "Phạm Minh Mây" },
  residentAn: { phone: "0900000010", password: "cudan123", fullName: "Trần Văn An" },
  residentBinh: { phone: "0900000011", password: "cudan123", fullName: "Lê Thị Bình" },
} as const;

/** Tạo tài khoản BQL đầu tiên khi triển khai thật (ADMIN_PHONE / ADMIN_PASSWORD). */
export function ensureAdmin(db: DB, admin: { phone: string; password: string }): void {
  const exists = db.prepare("SELECT 1 FROM users WHERE role = 'admin' LIMIT 1").get();
  if (exists) return;
  db.prepare(
    "INSERT INTO users (phone, password_hash, full_name, role) VALUES (?, ?, 'Ban Quản Lý', 'admin')",
  ).run(admin.phone, hashPassword(admin.password));
}

/** Nạp dữ liệu mẫu nếu database còn trống. Trả về `true` nếu đã nạp. */
export function seedDemoData(db: DB): boolean {
  const { count } = db.prepare("SELECT COUNT(*) AS count FROM users").get() as { count: number };
  if (count > 0) return false;

  transaction(db, () => {
    const insertUser = db.prepare(
      "INSERT INTO users (phone, password_hash, full_name, role, apartment) VALUES (?, ?, ?, ?, ?)",
    );
    const user = (a: { phone: string; password: string; fullName: string }, role: string, apt: string | null) =>
      Number(insertUser.run(a.phone, hashPassword(a.password), a.fullName, role, apt).lastInsertRowid);

    const admin = user(DEMO_ACCOUNTS.admin, "admin", null);
    const lan = user(DEMO_ACCOUNTS.merchantLan, "merchant", "B-03.02");
    const may = user(DEMO_ACCOUNTS.merchantMay, "merchant", "A-01.05");
    const an = user(DEMO_ACCOUNTS.residentAn, "resident", "A-12.05");
    const binh = user(DEMO_ACCOUNTS.residentBinh, "resident", "B-08.11");

    db.prepare(
      "INSERT INTO building_info (id, name, address, description, amenities) VALUES (1, ?, ?, ?, ?)",
    ).run(
      "Chung cư Hoa Sen Residence",
      "88 Đường Số 9, Phường An Phú, TP. Thủ Đức, TP. Hồ Chí Minh",
      "2 block A & B, 25 tầng, 480 căn hộ. Văn phòng BQL đặt tại sảnh block A, mở cửa 7:30 – 20:00 hằng ngày.",
      JSON.stringify(["Hồ bơi", "Phòng gym", "Sân chơi trẻ em", "Bãi đỗ xe hầm", "Siêu thị mini", "Bảo vệ 24/7"]),
    );
    const contact = db.prepare(
      "INSERT INTO contacts (name, phone, description, sort_order) VALUES (?, ?, ?, ?)",
    );
    [
      ["Văn phòng Ban Quản Lý", "02838000001", "7:30 – 20:00 hằng ngày"],
      ["Bảo vệ / An ninh", "0909000111", "Trực 24/7"],
      ["Kỹ thuật (điện, nước)", "0909000222", "Sự cố khẩn cấp 24/7"],
      ["Lễ tân sảnh A", "0909000333", "6:00 – 22:00"],
    ].forEach(([name, phone, description], i) => contact.run(name, phone, description, i));

    const announcement = db.prepare(
      "INSERT INTO announcements (title, content, category, pinned, author_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    );
    const ann = (title: string, content: string, category: string, pinned: boolean, days: number) =>
      announcement.run(title, content, category, pinned ? 1 : 0, admin, at(days), at(days));
    ann(
      "Tạm ngưng cấp nước block B ngày Chủ nhật",
      "Để vệ sinh bể nước định kỳ, block B sẽ tạm ngưng cấp nước từ 8:00 đến 14:00 Chủ nhật tuần này. Cư dân vui lòng chủ động trữ nước. Xin cảm ơn!",
      "maintenance",
      true,
      -1,
    );
    ann(
      "Thông báo phí quản lý tháng này",
      "Phí quản lý: 12.000đ/m². Cư dân vui lòng thanh toán trước ngày 10 qua chuyển khoản hoặc tại văn phòng BQL. Ghi chú nội dung: Mã căn hộ + tháng.",
      "fee",
      false,
      -3,
    );
    ann(
      "Ngày hội cư dân cuối tuần",
      "Chương trình giao lưu, chợ phiên nội khu và trò chơi cho thiếu nhi tại sân trung tâm, 16:00 – 20:00 thứ Bảy. Mời toàn thể cư dân tham gia!",
      "event",
      false,
      -4,
    );
    ann(
      "Diễn tập phòng cháy chữa cháy",
      "BQL phối hợp đơn vị PCCC tổ chức diễn tập lúc 9:00 thứ Năm. Chuông báo cháy sẽ kêu thử trong khoảng 5 phút, cư dân không cần di tản.",
      "security",
      false,
      -6,
    );

    const insertShop = db.prepare(
      `INSERT INTO shops (owner_id, name, description, category, phone, location, opening_hours)
       VALUES (?, ?, ?, ?, ?, ?, ?)`,
    );
    const shop = (...args: [number, string, string, string, string, string, string]) =>
      Number(insertShop.run(...args).lastInsertRowid);
    const bep = shop(lan, "Bếp Nhà Lan", "Cơm nhà nấu mỗi ngày, giao tận cửa căn hộ trong 20 phút.", "food", DEMO_ACCOUNTS.merchantLan.phone, "Căn B-03.02", "10:00 – 20:00");
    const tra = shop(may, "Trà Sữa Mây", "Trà sữa, trà trái cây, cà phê muối. Topping làm mới mỗi sáng.", "drink", DEMO_ACCOUNTS.merchantMay.phone, "Kiot 3 – Sảnh A", "8:00 – 22:00");
    const tap = shop(may, "Tạp Hoá Mây Mart", "Rau củ, trứng, sữa, đồ dùng gia đình. Đặt trước 9h giao trong buổi sáng.", "grocery", DEMO_ACCOUNTS.merchantMay.phone, "Kiot 4 – Sảnh A", "6:30 – 21:30");
    const giat = shop(lan, "Giặt Sấy Sạch Thơm", "Nhận đồ tận căn, giặt sấy gấp gọn, trả trong 24h.", "service", DEMO_ACCOUNTS.merchantLan.phone, "Tầng hầm B1", "7:00 – 19:00");

    const insertProduct = db.prepare(
      "INSERT INTO products (shop_id, name, description, price) VALUES (?, ?, ?, ?)",
    );
    const product = (shopId: number, name: string, description: string, price: number) =>
      Number(insertProduct.run(shopId, name, description, price).lastInsertRowid);
    const comSuon = product(bep, "Cơm sườn nướng", "Sườn cốt lết nướng than, kèm canh và đồ chua", 45_000);
    const comGa = product(bep, "Cơm gà xối mỡ", "Gà giòn, cơm chiên nước mắm", 42_000);
    product(bep, "Canh chua cá lóc", "Phần 2 người ăn", 55_000);
    product(bep, "Chè đậu xanh", "Ly 500ml", 15_000);
    const trasua = product(tra, "Trà sữa trân châu đường đen", "Size L", 35_000);
    const caphe = product(tra, "Cà phê muối", "Size M", 29_000);
    product(tra, "Trà đào cam sả", "Size L", 32_000);
    product(tap, "Trứng gà (vỉ 10)", "Trứng gà sạch", 32_000);
    product(tap, "Rau cải ngọt (500g)", "Rau Đà Lạt", 18_000);
    product(tap, "Sữa tươi không đường 1L", "", 38_000);
    product(giat, "Giặt sấy (theo kg)", "Tối thiểu 3kg", 18_000);
    product(giat, "Giặt chăn mền", "Một bộ", 60_000);

    const insertOrder = db.prepare(
      `INSERT INTO orders (resident_id, shop_id, status, delivery_apartment, contact_phone, note, total, created_at, updated_at)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    );
    const insertItem = db.prepare(
      "INSERT INTO order_items (order_id, product_id, product_name, unit_price, quantity) VALUES (?, ?, ?, ?, ?)",
    );
    const productInfo = db.prepare("SELECT name, price FROM products WHERE id = ?");
    const order = (
      resident: { id: number; apt: string; phone: string },
      shopId: number,
      status: string,
      items: [number, number][],
      days: number,
      note = "",
    ) => {
      const lines = items.map(([id, qty]) => ({ id, qty, ...(productInfo.get(id) as { name: string; price: number }) }));
      const total = lines.reduce((s, l) => s + l.price * l.qty, 0);
      const id = Number(
        insertOrder.run(resident.id, shopId, status, resident.apt, resident.phone, note, total, at(days), at(days)).lastInsertRowid,
      );
      for (const l of lines) insertItem.run(id, l.id, l.name, l.price, l.qty);
      return id;
    };
    const residentAn = { id: an, apt: "A-12.05", phone: DEMO_ACCOUNTS.residentAn.phone };
    const residentBinh = { id: binh, apt: "B-08.11", phone: DEMO_ACCOUNTS.residentBinh.phone };

    const o1 = order(residentAn, bep, "completed", [[comSuon, 2]], -5);
    const o2 = order(residentAn, tra, "completed", [[trasua, 1], [caphe, 1]], -3);
    const o3 = order(residentBinh, bep, "completed", [[comGa, 1]], -2);
    order(residentAn, bep, "pending", [[comGa, 1], [comSuon, 1]], 0, "Ít cơm, nhiều rau giúp mình nhé");

    const insertReview = db.prepare(
      "INSERT INTO reviews (shop_id, order_id, resident_id, rating, comment, reply, replied_at, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    );
    insertReview.run(bep, o1, an, 5, "Sườn mềm, giao nhanh, còn nóng hổi. Sẽ ủng hộ dài dài!", "Cảm ơn anh An đã ủng hộ Bếp ạ!", at(-4), at(-5));
    insertReview.run(tra, o2, an, 4, "Trà sữa ngon, cà phê muối hơi ngọt.", null, null, at(-3));
    insertReview.run(bep, o3, binh, 4, "Gà giòn, cơm vừa ăn.", null, null, at(-2));

    const insertPromo = db.prepare(
      `INSERT INTO promotions (shop_id, created_by, title, content, discount_label, starts_at, ends_at, status)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?)`,
    );
    insertPromo.run(tra, may, "Thứ Tư vui vẻ – Mua 1 tặng 1", "Áp dụng cho trà sữa size L mỗi thứ Tư.", "Mua 1 tặng 1", at(-2), at(30), "approved");
    insertPromo.run(bep, lan, "Freeship cho đơn từ 2 suất", "Bếp Nhà Lan miễn phí giao hàng trong toà nhà cho đơn từ 2 suất cơm.", "Freeship", at(-1), at(14), "approved");
    insertPromo.run(null, admin, "Chợ phiên cư dân cuối tháng", "Cư dân có nhu cầu đăng ký gian hàng vui lòng liên hệ văn phòng BQL trước thứ Sáu.", "Miễn phí gian hàng", at(-1), at(10), "approved");
    insertPromo.run(tap, may, "Giảm 10% rau củ buổi sáng", "Đặt trước 9h sáng giảm 10% tất cả rau củ.", "-10%", at(0), at(20), "pending");
  });
  return true;
}
