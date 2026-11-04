export type Role = "resident" | "merchant" | "admin";

export interface User {
  id: number;
  phone: string;
  fullName: string;
  role: Role;
  apartment: string | null;
  createdAt: string;
}

export interface Session {
  token: string;
  user: User;
}

export interface Contact {
  id: number;
  name: string;
  phone: string;
  description: string;
}

export interface Building {
  name: string;
  address: string;
  description: string;
  amenities: string[];
  contacts: Contact[];
}

export type AnnouncementCategory = "general" | "maintenance" | "event" | "fee" | "security";

export interface Announcement {
  id: number;
  title: string;
  content: string;
  category: AnnouncementCategory;
  pinned: boolean;
  createdAt: string;
  updatedAt: string;
  authorName: string | null;
}

export type ShopCategory = "food" | "drink" | "grocery" | "service" | "other";

export interface Shop {
  id: number;
  ownerId: number;
  name: string;
  description: string;
  category: ShopCategory;
  phone: string;
  location: string;
  openingHours: string;
  imageUrl: string | null;
  isActive: boolean;
  createdAt: string;
  ratingAvg: number;
  reviewCount: number;
}

export interface Product {
  id: number;
  shopId: number;
  name: string;
  description: string;
  price: number;
  imageUrl: string | null;
  isAvailable: boolean;
}

export interface ShopDetail extends Shop {
  products: Product[];
  canManage: boolean;
}

export type OrderStatus = "pending" | "confirmed" | "delivering" | "completed" | "cancelled";
export type PaymentMethod = "cod" | "transfer";

export interface OrderItem {
  productId: number | null;
  productName: string;
  unitPrice: number;
  quantity: number;
}

export interface Order {
  id: number;
  shopId: number;
  shopName: string;
  shopPhone: string;
  residentId: number;
  residentName: string;
  status: OrderStatus;
  deliveryApartment: string;
  contactPhone: string;
  note: string;
  paymentMethod: PaymentMethod;
  total: number;
  createdAt: string;
  updatedAt: string;
  review: { id: number; rating: number } | null;
  items: OrderItem[];
  viewerIsShop: boolean;
  allowedStatuses: OrderStatus[];
  canReview: boolean;
}

export interface Review {
  id: number;
  shopId: number;
  orderId: number;
  rating: number;
  comment: string;
  reply: string | null;
  repliedAt: string | null;
  createdAt: string;
  residentName: string;
}

export interface ReviewSummary {
  items: Review[];
  total: number;
  average: number;
  distribution: { rating: number; count: number }[];
}

export type PromotionStatus = "pending" | "approved" | "rejected";

export interface Promotion {
  id: number;
  shopId: number | null;
  shopName: string | null;
  shopCategory: ShopCategory | null;
  title: string;
  content: string;
  discountLabel: string;
  imageUrl: string | null;
  startsAt: string;
  endsAt: string;
  status: PromotionStatus;
  createdAt: string;
  createdByName: string | null;
}
