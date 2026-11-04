import { useDeferredValue, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { api } from "../api";
import { ShopCard } from "../components/cards";
import { Icon } from "../components/Icon";
import { Chips, Empty, ErrorState, Loading, Page, QtyStepper, ShopAvatar, Stars } from "../components/ui";
import { fold, formatDate, formatVnd, SHOP_CATEGORIES } from "../format";
import { useAsync } from "../hooks";
import { useCart } from "../state/cart";
import type { ReviewSummary, Shop, ShopCategory, ShopDetail } from "../types";

type Filter = ShopCategory | "all";
const FILTERS: { value: Filter; label: string }[] = [
  { value: "all", label: "Tất cả" },
  ...Object.entries(SHOP_CATEGORIES).map(([value, c]) => ({ value: value as Filter, label: `${c.icon} ${c.label}` })),
];

export function CartButton() {
  const cart = useCart();
  return (
    <Link to="/cart" className="icon-btn cart-btn" aria-label={`Giỏ hàng (${cart.count} món)`}>
      <Icon name="cart" />
      {cart.count > 0 && <span className="cart-count">{cart.count}</span>}
    </Link>
  );
}

export function ShopsPage() {
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const deferredQuery = useDeferredValue(query);
  const { data, error, loading, reload } = useAsync(
    () => api<Shop[]>("/shops", { query: { category: filter === "all" ? undefined : filter } }),
    [filter],
  );
  // Lọc tại chỗ để gõ tới đâu thấy tới đó; danh sách cửa hàng trong một toà nhà không lớn.
  const needle = fold(deferredQuery.trim());
  const shops = data?.filter((s) => !needle || fold(`${s.name} ${s.description}`).includes(needle));

  return (
    <Page title="Cửa hàng nội khu" action={<CartButton />}>
      <label className="search">
        <Icon name="search" size={18} />
        <input
          type="search"
          placeholder="Tìm cửa hàng, món ăn…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          aria-label="Tìm cửa hàng"
        />
      </label>
      <Chips options={FILTERS} value={filter} onChange={setFilter} />
      {loading && !data && <Loading />}
      {error && <ErrorState error={error} onRetry={reload} />}
      {shops?.length === 0 && <Empty icon="🔍" title="Không tìm thấy cửa hàng phù hợp" />}
      <div className="stack">
        {shops?.map((s) => (
          <ShopCard key={s.id} shop={s} />
        ))}
      </div>
    </Page>
  );
}

export function ShopPage() {
  const { id } = useParams();
  const [tab, setTab] = useState<"menu" | "reviews">("menu");
  const cart = useCart();
  const shop = useAsync(() => api<ShopDetail>(`/shops/${id}`), [id]);
  const reviews = useAsync(() => api<ReviewSummary>(`/shops/${id}/reviews`), [id]);

  const s = shop.data;
  const cartHere = cart.shop?.id === s?.id && cart.count > 0;

  return (
    <Page title={s?.name ?? "Cửa hàng"} back action={<CartButton />}>
      {shop.loading && !s && <Loading />}
      {shop.error && <ErrorState error={shop.error} onRetry={shop.reload} />}
      {s && (
        <>
          <section className="card shop-hero">
            <ShopAvatar category={s.category} size={64} />
            <div>
              <h2>{s.name}</h2>
              <div className="rating-line">
                <Stars value={s.ratingAvg} size="sm" />
                <span>
                  {s.reviewCount ? `${s.ratingAvg.toFixed(1)} · ${s.reviewCount} đánh giá` : "Chưa có đánh giá"}
                </span>
              </div>
            </div>
            {s.description && <p className="shop-desc">{s.description}</p>}
            <ul className="shop-facts">
              {s.location && (
                <li>
                  <Icon name="location" size={16} /> {s.location}
                </li>
              )}
              {s.openingHours && (
                <li>
                  <Icon name="clock" size={16} /> {s.openingHours}
                </li>
              )}
              {s.phone && (
                <li>
                  <Icon name="phone" size={16} /> <a href={`tel:${s.phone}`}>{s.phone}</a>
                </li>
              )}
            </ul>
            {!s.isActive && <p className="notice">Cửa hàng đang tạm nghỉ, chưa nhận đơn.</p>}
            {s.canManage && (
              <Link to={`/merchant?shop=${s.id}`} className="btn btn-outline btn-sm">
                Quản lý cửa hàng
              </Link>
            )}
          </section>

          <Chips
            options={[
              { value: "menu", label: `Sản phẩm (${s.products.length})` },
              { value: "reviews", label: `Đánh giá (${reviews.data?.total ?? s.reviewCount})` },
            ]}
            value={tab}
            onChange={setTab}
          />

          {tab === "menu" && (
            <div className="card list">
              {s.products.length === 0 && <Empty title="Cửa hàng chưa đăng sản phẩm" />}
              {s.products.map((p) => {
                const qty = cart.shop?.id === s.id ? cart.quantityOf(p.id) : 0;
                return (
                  <div key={p.id} className={`product-row${p.isAvailable ? "" : " product-off"}`}>
                    <div className="product-info">
                      <strong>{p.name}</strong>
                      {p.description && <span className="muted small">{p.description}</span>}
                      <span className="price">{formatVnd(p.price)}</span>
                    </div>
                    {!p.isAvailable ? (
                      <span className="muted small">Hết hàng</span>
                    ) : qty > 0 ? (
                      <QtyStepper value={qty} onChange={(v) => cart.setQuantity(p.id, v)} label={p.name} />
                    ) : (
                      <button
                        className="add-btn"
                        onClick={() => cart.add(s, p)}
                        disabled={!s.isActive}
                        aria-label={`Thêm ${p.name} vào giỏ`}
                      >
                        <Icon name="plus" size={20} />
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          )}

          {tab === "reviews" && <ReviewList summary={reviews.data} loading={reviews.loading} />}

          {cartHere && (
            <Link to="/cart" className="cart-bar">
              <span>
                <Icon name="cart" size={18} /> {cart.count} món
              </span>
              <strong>Xem giỏ hàng · {formatVnd(cart.total)}</strong>
            </Link>
          )}
        </>
      )}
    </Page>
  );
}

function ReviewList({ summary, loading }: { summary?: ReviewSummary; loading: boolean }) {
  if (loading && !summary) return <Loading />;
  if (!summary || summary.total === 0) {
    return <Empty icon="⭐" title="Chưa có đánh giá">Đặt hàng và là người đầu tiên đánh giá cửa hàng nhé!</Empty>;
  }
  const max = Math.max(...summary.distribution.map((d) => d.count), 1);
  return (
    <>
      <section className="card review-summary">
        <div className="review-score">
          <strong>{summary.average.toFixed(1)}</strong>
          <Stars value={summary.average} size="sm" />
          <span className="muted small">{summary.total} đánh giá</span>
        </div>
        <ul className="review-bars">
          {[...summary.distribution].reverse().map((d) => (
            <li key={d.rating}>
              <span>{d.rating}★</span>
              <span className="bar">
                <span style={{ width: `${(d.count / max) * 100}%` }} />
              </span>
              <span className="muted">{d.count}</span>
            </li>
          ))}
        </ul>
      </section>
      <div className="stack">
        {summary.items.map((r) => (
          <article key={r.id} className="card review">
            <div className="review-head">
              <strong>{r.residentName}</strong>
              <span className="muted small">{formatDate(r.createdAt)}</span>
            </div>
            <Stars value={r.rating} size="sm" />
            {r.comment && <p>{r.comment}</p>}
            {r.reply && (
              <div className="review-reply">
                <strong>Phản hồi của cửa hàng</strong>
                <p>{r.reply}</p>
              </div>
            )}
          </article>
        ))}
      </div>
    </>
  );
}
