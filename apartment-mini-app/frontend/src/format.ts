import type {
  AnnouncementCategory,
  OrderStatus,
  PaymentMethod,
  PromotionStatus,
  Role,
  ShopCategory,
} from "./types";

export const SHOP_CATEGORIES: Record<ShopCategory, { label: string; icon: string }> = {
  food: { label: "Đồ ăn", icon: "🍱" },
  drink: { label: "Đồ uống", icon: "🧋" },
  grocery: { label: "Tạp hoá", icon: "🛒" },
  service: { label: "Dịch vụ", icon: "🧺" },
  other: { label: "Khác", icon: "🏪" },
};

export const ANNOUNCEMENT_CATEGORIES: Record<AnnouncementCategory, { label: string; icon: string }> = {
  general: { label: "Chung", icon: "📢" },
  maintenance: { label: "Bảo trì", icon: "🛠️" },
  event: { label: "Sự kiện", icon: "🎉" },
  fee: { label: "Phí dịch vụ", icon: "💳" },
  security: { label: "An ninh", icon: "🛡️" },
};

export type Tone = "warning" | "info" | "success" | "muted" | "danger";

export const ORDER_STATUSES: Record<OrderStatus, { label: string; tone: Tone; action: string }> = {
  pending: { label: "Chờ xác nhận", tone: "warning", action: "Chờ xác nhận" },
  confirmed: { label: "Đã xác nhận", tone: "info", action: "Xác nhận đơn" },
  delivering: { label: "Đang giao", tone: "info", action: "Bắt đầu giao" },
  completed: { label: "Hoàn thành", tone: "success", action: "Đã giao xong" },
  cancelled: { label: "Đã huỷ", tone: "muted", action: "Huỷ đơn" },
};

export const PROMOTION_STATUSES: Record<PromotionStatus, { label: string; tone: Tone }> = {
  pending: { label: "Chờ duyệt", tone: "warning" },
  approved: { label: "Đang hiển thị", tone: "success" },
  rejected: { label: "Bị từ chối", tone: "danger" },
};

export const PAYMENT_METHODS: Record<PaymentMethod, string> = {
  cod: "Tiền mặt khi nhận hàng",
  transfer: "Chuyển khoản",
};

export const ROLES: Record<Role, string> = {
  resident: "Cư dân",
  merchant: "Chủ cửa hàng",
  admin: "Ban quản lý",
};

const vnd = new Intl.NumberFormat("vi-VN");
export const formatVnd = (amount: number) => `${vnd.format(amount)}đ`;

const dateFmt = new Intl.DateTimeFormat("vi-VN", { day: "2-digit", month: "2-digit", year: "numeric" });
const timeFmt = new Intl.DateTimeFormat("vi-VN", { hour: "2-digit", minute: "2-digit" });

export const formatDate = (iso: string) => dateFmt.format(new Date(iso));
export const formatDateTime = (iso: string) => `${timeFmt.format(new Date(iso))} · ${dateFmt.format(new Date(iso))}`;

export function timeAgo(iso: string): string {
  const seconds = Math.round((Date.now() - new Date(iso).getTime()) / 1000);
  if (seconds < 60) return "Vừa xong";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} phút trước`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} giờ trước`;
  const days = Math.round(hours / 24);
  if (days < 7) return `${days} ngày trước`;
  return formatDate(iso);
}

/** "yyyy-mm-dd" theo giờ địa phương, dùng cho <input type="date">. */
export function toDateInput(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** Bỏ dấu để tìm kiếm tiếng Việt. */
export function fold(value: string): string {
  return value.normalize("NFD").replace(/[̀-ͯ]/g, "").replace(/đ/gi, "d").toLowerCase();
}
