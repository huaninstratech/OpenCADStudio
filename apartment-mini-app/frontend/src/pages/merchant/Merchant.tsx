import { useState, type FormEvent } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { api, errorMessage } from "../../api";
import { OrderCard } from "../../components/cards";
import { PromotionForm } from "../../components/PromotionForm";
import { Badge, Chips, Empty, ErrorState, Field, Loading, Page, Stars } from "../../components/ui";
import { formatDate, formatVnd, PROMOTION_STATUSES, SHOP_CATEGORIES } from "../../format";
import { useAsync } from "../../hooks";
import { useToast } from "../../state/toast";
import type { Order, Product, Promotion, ReviewSummary, Shop, ShopCategory, ShopDetail } from "../../types";
import { matchesFilter } from "../Orders";

type Tab = "orders" | "products" | "promotions" | "reviews" | "settings";
const TABS: { value: Tab; label: string }[] = [
  { value: "orders", label: "Đơn hàng" },
  { value: "products", label: "Sản phẩm" },
  { value: "promotions", label: "Quảng bá" },
  { value: "reviews", label: "Đánh giá" },
  { value: "settings", label: "Cửa hàng" },
];

export function MerchantPage() {
  const [params, setParams] = useSearchParams();
  const shops = useAsync(() => api<Shop[]>("/shops/mine"), []);
  const [tab, setTab] = useState<Tab>("orders");

  const selectedId = Number(params.get("shop")) || shops.data?.[0]?.id;
  const shop = shops.data?.find((s) => s.id === selectedId) ?? shops.data?.[0];

  return (
    <Page title="Quản lý cửa hàng" back="/account">
      {shops.loading && !shops.data && <Loading />}
      {shops.error && <ErrorState error={shops.error} onRetry={shops.reload} />}
      {shops.data?.length === 0 && (
        <>
          <Empty icon="🏪" title="Bạn chưa có cửa hàng">
            Mở cửa hàng để bán cho cư dân trong toà nhà.
          </Empty>
          <ShopForm onSaved={(s) => { shops.reload(); setParams({ shop: String(s.id) }); }} />
        </>
      )}
      {shop && (
        <>
          {shops.data!.length > 1 && (
            <Chips
              options={shops.data!.map((s) => ({ value: String(s.id), label: s.name }))}
              value={String(shop.id)}
              onChange={(v) => setParams({ shop: v }, { replace: true })}
            />
          )}
          <Link to={`/shops/${shop.id}`} className="card shop-mini">
            <strong>{shop.name}</strong>
            <span className="muted small">
              {shop.isActive ? "Đang mở bán" : "Tạm nghỉ"} · {shop.reviewCount} đánh giá
            </span>
          </Link>
          <Chips options={TABS} value={tab} onChange={setTab} />
          {tab === "orders" && <ShopOrders shopId={shop.id} />}
          {tab === "products" && <ShopProducts shopId={shop.id} />}
          {tab === "promotions" && <ShopPromotions shopId={shop.id} />}
          {tab === "reviews" && <ShopReviews shopId={shop.id} />}
          {tab === "settings" && <ShopForm key={shop.id} shop={shop} onSaved={shops.reload} />}
        </>
      )}
    </Page>
  );
}

function ShopOrders({ shopId }: { shopId: number }) {
  const [filter, setFilter] = useState<"active" | "all">("active");
  const { data, error, loading, reload } = useAsync(() => api<Order[]>("/orders", { query: { as: "merchant" } }), []);
  const orders = data?.filter((o) => o.shopId === shopId && matchesFilter(o, filter));
  return (
    <>
      <div className="row-between">
        <Chips
          options={[
            { value: "active", label: "Cần xử lý" },
            { value: "all", label: "Tất cả" },
          ]}
          value={filter}
          onChange={setFilter}
        />
        <button className="btn btn-outline btn-sm" onClick={reload}>
          Làm mới
        </button>
      </div>
      {loading && !data && <Loading />}
      {error && <ErrorState error={error} onRetry={reload} />}
      {orders?.length === 0 && <Empty icon="🎉" title="Không có đơn nào cần xử lý" />}
      <div className="stack">
        {orders?.map((o) => (
          <OrderCard key={o.id} order={o} to={`/orders/${o.id}`} />
        ))}
      </div>
    </>
  );
}

function ShopProducts({ shopId }: { shopId: number }) {
  const toast = useToast();
  const { data, error, loading, reload } = useAsync(() => api<ShopDetail>(`/shops/${shopId}`), [shopId]);
  const [form, setForm] = useState({ name: "", price: "", description: "" });
  const [formError, setFormError] = useState<string | null>(null);

  const update = async (product: Product, patch: Partial<Product>) => {
    try {
      await api(`/products/${product.id}`, { method: "PATCH", body: patch });
      reload();
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };

  const remove = async (product: Product) => {
    if (!window.confirm(`Xoá "${product.name}"?`)) return;
    try {
      await api(`/products/${product.id}`, { method: "DELETE" });
      reload();
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };

  const create = async (e: FormEvent) => {
    e.preventDefault();
    setFormError(null);
    try {
      await api(`/shops/${shopId}/products`, {
        method: "POST",
        body: { name: form.name, price: Number(form.price), description: form.description },
      });
      setForm({ name: "", price: "", description: "" });
      toast("Đã thêm sản phẩm");
      reload();
    } catch (err) {
      setFormError(errorMessage(err));
    }
  };

  return (
    <>
      {loading && !data && <Loading />}
      {error && <ErrorState error={error} onRetry={reload} />}
      {data && (
        <div className="card list">
          {data.products.length === 0 && <Empty title="Chưa có sản phẩm" />}
          {data.products.map((p) => (
            <div key={p.id} className={`product-row${p.isAvailable ? "" : " product-off"}`}>
              <div className="product-info">
                <strong>{p.name}</strong>
                <span className="price">{formatVnd(p.price)}</span>
              </div>
              <div className="row-actions">
                <label className="switch">
                  <input type="checkbox" checked={p.isAvailable} onChange={(e) => update(p, { isAvailable: e.target.checked })} />
                  <span>{p.isAvailable ? "Còn hàng" : "Hết hàng"}</span>
                </label>
                <button className="btn btn-ghost btn-sm" onClick={() => remove(p)} aria-label={`Xoá ${p.name}`}>
                  Xoá
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
      <form className="card form" onSubmit={create}>
        <h2>Thêm sản phẩm</h2>
        <Field label="Tên sản phẩm">
          {(id) => <input id={id} required maxLength={120} value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />}
        </Field>
        <Field label="Giá (VNĐ)">
          {(id) => (
            <input id={id} required type="number" inputMode="numeric" min={0} step={1000} value={form.price} onChange={(e) => setForm({ ...form, price: e.target.value })} />
          )}
        </Field>
        <Field label="Mô tả">
          {(id) => <input id={id} maxLength={1000} value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} />}
        </Field>
        {formError && <p className="form-error" role="alert">{formError}</p>}
        <button className="btn btn-primary btn-block">Thêm sản phẩm</button>
      </form>
    </>
  );
}

export function PromotionList({
  promos,
  onChanged,
  moderate = false,
}: {
  promos: Promotion[];
  onChanged: () => void;
  moderate?: boolean;
}) {
  const toast = useToast();
  const act = async (promo: Promotion, action: "approved" | "rejected" | "delete") => {
    if (action === "delete" && !window.confirm(`Xoá tin "${promo.title}"?`)) return;
    try {
      if (action === "delete") await api(`/promotions/${promo.id}`, { method: "DELETE" });
      else await api(`/promotions/${promo.id}/status`, { method: "PATCH", body: { status: action } });
      toast(action === "approved" ? "Đã duyệt tin" : action === "rejected" ? "Đã từ chối tin" : "Đã xoá tin");
      onChanged();
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };

  if (promos.length === 0) return <Empty icon="📣" title="Chưa có tin quảng bá" />;
  return (
    <div className="stack">
      {promos.map((p) => {
        const status = PROMOTION_STATUSES[p.status];
        return (
          <article key={p.id} className="card promo-admin">
            <div className="row-between">
              <strong>{p.title}</strong>
              <Badge tone={status.tone}>{status.label}</Badge>
            </div>
            <p className="muted small">
              {p.shopName ?? "Ban quản lý"} · {formatDate(p.startsAt)} – {formatDate(p.endsAt)}
              {p.discountLabel && ` · ${p.discountLabel}`}
            </p>
            {p.content && <p className="small">{p.content}</p>}
            <div className="row-actions">
              {moderate && p.status !== "approved" && (
                <button className="btn btn-primary btn-sm" onClick={() => act(p, "approved")}>
                  Duyệt
                </button>
              )}
              {moderate && p.status === "pending" && (
                <button className="btn btn-outline btn-sm" onClick={() => act(p, "rejected")}>
                  Từ chối
                </button>
              )}
              <button className="btn btn-ghost btn-sm" onClick={() => act(p, "delete")}>
                Xoá
              </button>
            </div>
          </article>
        );
      })}
    </div>
  );
}

function ShopPromotions({ shopId }: { shopId: number }) {
  const toast = useToast();
  const { data, error, loading, reload } = useAsync(() => api<Promotion[]>("/promotions/manage"), []);
  return (
    <>
      <PromotionForm
        shopId={shopId}
        note="Tin của cửa hàng sẽ hiển thị cho cư dân sau khi Ban quản lý duyệt."
        onCreated={(p) => {
          toast(p.status === "approved" ? "Đã đăng tin" : "Đã gửi tin, chờ BQL duyệt");
          reload();
        }}
      />
      {loading && !data && <Loading />}
      {error && <ErrorState error={error} onRetry={reload} />}
      {data && <PromotionList promos={data.filter((p) => p.shopId === shopId)} onChanged={reload} />}
    </>
  );
}

function ShopReviews({ shopId }: { shopId: number }) {
  const { data, error, loading, reload } = useAsync(
    () => api<ReviewSummary>(`/shops/${shopId}/reviews`, { query: { limit: 100 } }),
    [shopId],
  );
  if (loading && !data) return <Loading />;
  if (error) return <ErrorState error={error} onRetry={reload} />;
  if (!data?.items.length) return <Empty icon="⭐" title="Chưa có đánh giá" />;
  return (
    <div className="stack">
      <p className="muted small">
        Trung bình {data.average.toFixed(1)}★ từ {data.total} đánh giá
      </p>
      {data.items.map((r) => (
        <article key={r.id} className="card review">
          <div className="review-head">
            <strong>{r.residentName}</strong>
            <span className="muted small">{formatDate(r.createdAt)}</span>
          </div>
          <Stars value={r.rating} size="sm" />
          {r.comment && <p>{r.comment}</p>}
          {r.reply ? (
            <div className="review-reply">
              <strong>Phản hồi của bạn</strong>
              <p>{r.reply}</p>
            </div>
          ) : (
            <ReplyForm reviewId={r.id} onDone={reload} />
          )}
        </article>
      ))}
    </div>
  );
}

function ReplyForm({ reviewId, onDone }: { reviewId: number; onDone: () => void }) {
  const toast = useToast();
  const [reply, setReply] = useState("");
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await api(`/reviews/${reviewId}/reply`, { method: "POST", body: { reply } });
      toast("Đã gửi phản hồi");
      onDone();
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };
  return (
    <form className="reply-form" onSubmit={submit}>
      <input required maxLength={1000} value={reply} onChange={(e) => setReply(e.target.value)} placeholder="Phản hồi khách hàng…" aria-label="Nội dung phản hồi" />
      <button className="btn btn-primary btn-sm">Gửi</button>
    </form>
  );
}

function ShopForm({ shop, onSaved }: { shop?: Shop; onSaved: (shop: Shop) => void }) {
  const toast = useToast();
  const [form, setForm] = useState({
    name: shop?.name ?? "",
    category: shop?.category ?? ("food" as ShopCategory),
    description: shop?.description ?? "",
    phone: shop?.phone ?? "",
    location: shop?.location ?? "",
    openingHours: shop?.openingHours ?? "",
    isActive: shop?.isActive ?? true,
  });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = (key: keyof typeof form) => (e: { target: { value: string } }) =>
    setForm((f) => ({ ...f, [key]: e.target.value }));

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const saved = await api<Shop>(shop ? `/shops/${shop.id}` : "/shops", {
        method: shop ? "PATCH" : "POST",
        body: form,
      });
      toast(shop ? "Đã lưu thông tin cửa hàng" : "Đã mở cửa hàng!");
      onSaved(saved);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="card form" onSubmit={submit}>
      <h2>{shop ? "Thông tin cửa hàng" : "Mở cửa hàng mới"}</h2>
      <Field label="Tên cửa hàng">
        {(id) => <input id={id} required maxLength={100} value={form.name} onChange={set("name")} />}
      </Field>
      <Field label="Danh mục">
        {(id) => (
          <select id={id} value={form.category} onChange={set("category")}>
            {Object.entries(SHOP_CATEGORIES).map(([value, c]) => (
              <option key={value} value={value}>
                {c.icon} {c.label}
              </option>
            ))}
          </select>
        )}
      </Field>
      <Field label="Giới thiệu">
        {(id) => <textarea id={id} rows={2} maxLength={1000} value={form.description} onChange={set("description")} />}
      </Field>
      <div className="grid-2">
        <Field label="Điện thoại">
          {(id) => <input id={id} type="tel" maxLength={20} value={form.phone} onChange={set("phone")} />}
        </Field>
        <Field label="Giờ mở cửa">
          {(id) => <input id={id} maxLength={100} value={form.openingHours} onChange={set("openingHours")} placeholder="8:00 – 21:00" />}
        </Field>
      </div>
      <Field label="Vị trí">
        {(id) => <input id={id} maxLength={150} value={form.location} onChange={set("location")} placeholder="Kiot 2 – Sảnh B" />}
      </Field>
      {shop && (
        <label className="switch">
          <input type="checkbox" checked={form.isActive} onChange={(e) => setForm((f) => ({ ...f, isActive: e.target.checked }))} />
          <span>{form.isActive ? "Đang nhận đơn" : "Tạm nghỉ (ẩn khỏi danh sách)"}</span>
        </label>
      )}
      {error && <p className="form-error" role="alert">{error}</p>}
      <button className="btn btn-primary btn-block" disabled={busy}>
        {busy ? "Đang lưu…" : shop ? "Lưu thay đổi" : "Mở cửa hàng"}
      </button>
    </form>
  );
}
