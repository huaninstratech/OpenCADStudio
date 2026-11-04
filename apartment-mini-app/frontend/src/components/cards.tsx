import { Link } from "react-router-dom";
import { ANNOUNCEMENT_CATEGORIES, formatDate, formatVnd, ORDER_STATUSES, SHOP_CATEGORIES, timeAgo } from "../format";
import type { Announcement, Order, Promotion, Shop } from "../types";
import { Icon } from "./Icon";
import { Badge, ShopAvatar, Stars } from "./ui";

export function ShopCard({ shop }: { shop: Shop }) {
  return (
    <Link to={`/shops/${shop.id}`} className="card shop-card">
      <ShopAvatar category={shop.category} />
      <div className="shop-card-body">
        <strong className="shop-name">{shop.name}</strong>
        <div className="rating-line">
          {shop.reviewCount > 0 ? (
            <>
              <Stars value={shop.ratingAvg} size="sm" />
              <span>
                {shop.ratingAvg.toFixed(1)} ({shop.reviewCount})
              </span>
            </>
          ) : (
            <span className="muted">Chưa có đánh giá</span>
          )}
          <span className="dot">·</span>
          <span className="muted">{SHOP_CATEGORIES[shop.category].label}</span>
        </div>
        {shop.location && (
          <span className="meta">
            <Icon name="location" size={14} /> {shop.location}
          </span>
        )}
      </div>
      <Icon name="chevron" size={18} />
    </Link>
  );
}

export function PromoCard({ promo, wide = false }: { promo: Promotion; wide?: boolean }) {
  const body = (
    <>
      <div className="promo-top">
        {promo.discountLabel && <span className="promo-label">{promo.discountLabel}</span>}
        <span className="promo-shop">{promo.shopName ?? "Ban quản lý"}</span>
      </div>
      <strong className="promo-title">{promo.title}</strong>
      {promo.content && <p className="promo-content">{promo.content}</p>}
      <span className="promo-date">Đến {formatDate(promo.endsAt)}</span>
    </>
  );
  const className = `promo-card${wide ? " promo-card-wide" : ""}${promo.shopId ? "" : " promo-card-bql"}`;
  return promo.shopId ? (
    <Link to={`/shops/${promo.shopId}`} className={className}>
      {body}
    </Link>
  ) : (
    <div className={className}>{body}</div>
  );
}

export function AnnouncementRow({ item }: { item: Announcement }) {
  const cat = ANNOUNCEMENT_CATEGORIES[item.category];
  return (
    <Link to={`/info/announcements/${item.id}`} className="card announcement-row">
      <span className={`ann-icon ann-${item.category}`} aria-hidden="true">
        {cat.icon}
      </span>
      <div className="announcement-body">
        <div className="announcement-meta">
          {item.pinned && (
            <span className="pinned">
              <Icon name="pin" size={13} /> Ghim
            </span>
          )}
          <span>{cat.label}</span>
          <span className="dot">·</span>
          <span>{timeAgo(item.createdAt)}</span>
        </div>
        <strong>{item.title}</strong>
        <p className="clamp-2">{item.content}</p>
      </div>
    </Link>
  );
}

export function OrderCard({ order, to }: { order: Order; to: string }) {
  const status = ORDER_STATUSES[order.status];
  const summary = order.items.map((i) => `${i.quantity}× ${i.productName}`).join(", ");
  return (
    <Link to={to} className="card order-card">
      <div className="order-card-head">
        <strong>{order.viewerIsShop ? `${order.residentName} · ${order.deliveryApartment}` : order.shopName}</strong>
        <Badge tone={status.tone}>{status.label}</Badge>
      </div>
      <p className="clamp-2 muted">{summary}</p>
      <div className="order-card-foot">
        <span className="muted">
          #{order.id} · {timeAgo(order.createdAt)}
        </span>
        <span className="order-total">{formatVnd(order.total)}</span>
      </div>
      {order.canReview && <span className="review-cta">★ Đánh giá đơn hàng</span>}
    </Link>
  );
}
