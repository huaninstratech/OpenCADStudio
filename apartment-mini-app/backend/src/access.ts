import type { AuthUser } from "./auth.js";
import type { DB } from "./db/index.js";
import { forbidden, notFound } from "./http.js";

export interface ShopRecord {
  id: number;
  owner_id: number;
  name: string;
  is_active: number;
}

export function getShopRecord(db: DB, id: number): ShopRecord {
  const shop = db
    .prepare("SELECT id, owner_id, name, is_active FROM shops WHERE id = ?")
    .get(id) as ShopRecord | undefined;
  if (!shop) throw notFound("cửa hàng");
  return shop;
}

export function canManageShop(user: AuthUser | undefined, shop: ShopRecord): boolean {
  return !!user && (user.role === "admin" || user.id === shop.owner_id);
}

/** Trả về cửa hàng nếu `user` là chủ cửa hàng hoặc BQL, ngược lại ném 403. */
export function requireShopManager(db: DB, user: AuthUser, shopId: number): ShopRecord {
  const shop = getShopRecord(db, shopId);
  if (!canManageShop(user, shop)) throw forbidden("Bạn không quản lý cửa hàng này");
  return shop;
}

/** Bỏ dấu tiếng Việt để tìm kiếm "bep" khớp với "Bếp". */
export function foldVietnamese(value: string): string {
  return value
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/đ/g, "d")
    .replace(/Đ/g, "D")
    .toLowerCase();
}
