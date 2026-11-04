import { useEffect, useState, type FormEvent } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api, errorMessage } from "../api";
import { OrderCard } from "../components/cards";
import { Chips, Empty, ErrorState, Field, Loading, Page, QtyStepper, StarInput, Stars } from "../components/ui";
import { formatDateTime, formatVnd, ORDER_STATUSES, PAYMENT_METHODS } from "../format";
import { useAsync } from "../hooks";
import { useAuth } from "../state/auth";
import { useCart } from "../state/cart";
import { useToast } from "../state/toast";
import type { Order, OrderStatus, PaymentMethod } from "../types";

export function CartPage() {
  const cart = useCart();
  const { user } = useAuth();
  const toast = useToast();
  const navigate = useNavigate();
  const [apartment, setApartment] = useState(user?.apartment ?? "");
  const [phone, setPhone] = useState(user?.phone ?? "");
  const [note, setNote] = useState("");
  const [payment, setPayment] = useState<PaymentMethod>("cod");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Thông tin người dùng có thể về sau lần render đầu (khi app vừa mở).
  useEffect(() => {
    if (!user) return;
    setApartment((a) => a || user.apartment || "");
    setPhone((p) => p || user.phone);
  }, [user]);

  if (!cart.shop || cart.lines.length === 0) {
    return (
      <Page title="Giỏ hàng" back>
        <Empty icon="🛒" title="Giỏ hàng trống">
          <Link to="/shops" className="btn btn-primary">
            Khám phá cửa hàng
          </Link>
        </Empty>
      </Page>
    );
  }

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setSubmitting(true);
    setError(null);
    try {
      const order = await api<Order>("/orders", {
        method: "POST",
        body: {
          shopId: cart.shop!.id,
          items: cart.lines.map((l) => ({ productId: l.product.id, quantity: l.quantity })),
          deliveryApartment: apartment.trim() || undefined,
          contactPhone: phone.trim() || undefined,
          note: note.trim(),
          paymentMethod: payment,
        },
      });
      cart.clear();
      toast("Đặt hàng thành công! Cửa hàng sẽ xác nhận sớm.");
      navigate(`/orders/${order.id}`, { replace: true });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Page title="Giỏ hàng" back>
      <section className="card">
        <div className="section-title compact">
          <h2>{cart.shop.name}</h2>
          <Link to={`/shops/${cart.shop.id}`} className="link">
            Thêm món
          </Link>
        </div>
        <div className="list">
          {cart.lines.map((l) => (
            <div key={l.product.id} className="product-row">
              <div className="product-info">
                <strong>{l.product.name}</strong>
                <span className="price">{formatVnd(l.product.price * l.quantity)}</span>
              </div>
              <QtyStepper value={l.quantity} onChange={(v) => cart.setQuantity(l.product.id, v)} label={l.product.name} />
            </div>
          ))}
        </div>
      </section>

      {!user ? (
        <section className="card center">
          <p>Đăng nhập để hoàn tất đặt hàng.</p>
          <Link to="/login?next=/cart" className="btn btn-primary btn-block">
            Đăng nhập
          </Link>
        </section>
      ) : (
        <form className="card form" onSubmit={submit}>
          <h2>Thông tin nhận hàng</h2>
          <Field label="Căn hộ nhận hàng">
            {(id) => (
              <input id={id} required value={apartment} onChange={(e) => setApartment(e.target.value)} placeholder="VD: A-12.05" maxLength={30} />
            )}
          </Field>
          <Field label="Số điện thoại liên hệ">
            {(id) => <input id={id} type="tel" required value={phone} onChange={(e) => setPhone(e.target.value)} />}
          </Field>
          <Field label="Ghi chú cho cửa hàng">
            {(id) => (
              <textarea id={id} rows={2} value={note} onChange={(e) => setNote(e.target.value)} maxLength={500} placeholder="Ít cay, giao trước 12h…" />
            )}
          </Field>
          <fieldset className="radio-group">
            <legend>Thanh toán</legend>
            {(Object.keys(PAYMENT_METHODS) as PaymentMethod[]).map((m) => (
              <label key={m} className={`radio${payment === m ? " radio-active" : ""}`}>
                <input type="radio" name="payment" value={m} checked={payment === m} onChange={() => setPayment(m)} />
                {PAYMENT_METHODS[m]}
              </label>
            ))}
          </fieldset>
          <div className="total-row">
            <span>Tổng cộng</span>
            <strong>{formatVnd(cart.total)}</strong>
          </div>
          {error && <p className="form-error" role="alert">{error}</p>}
          <button className="btn btn-primary btn-block" disabled={submitting}>
            {submitting ? "Đang đặt hàng…" : `Đặt hàng · ${formatVnd(cart.total)}`}
          </button>
        </form>
      )}
    </Page>
  );
}

type Filter = "all" | "active" | "completed" | "cancelled";
const FILTERS: { value: Filter; label: string }[] = [
  { value: "all", label: "Tất cả" },
  { value: "active", label: "Đang xử lý" },
  { value: "completed", label: "Hoàn thành" },
  { value: "cancelled", label: "Đã huỷ" },
];
const ACTIVE: OrderStatus[] = ["pending", "confirmed", "delivering"];

export function matchesFilter(order: Order, filter: Filter) {
  if (filter === "all") return true;
  if (filter === "active") return ACTIVE.includes(order.status);
  return order.status === filter;
}

export function OrdersPage() {
  const [filter, setFilter] = useState<Filter>("all");
  const { data, error, loading, reload } = useAsync(() => api<Order[]>("/orders"), []);
  const orders = data?.filter((o) => matchesFilter(o, filter));

  return (
    <Page title="Đơn hàng của tôi">
      <Chips options={FILTERS} value={filter} onChange={setFilter} />
      {loading && !data && <Loading />}
      {error && <ErrorState error={error} onRetry={reload} />}
      {orders?.length === 0 && (
        <Empty icon="🧾" title="Chưa có đơn hàng">
          <Link to="/shops" className="btn btn-primary">
            Đặt món ngay
          </Link>
        </Empty>
      )}
      <div className="stack">
        {orders?.map((o) => (
          <OrderCard key={o.id} order={o} to={`/orders/${o.id}`} />
        ))}
      </div>
    </Page>
  );
}

const STEPS: OrderStatus[] = ["pending", "confirmed", "delivering", "completed"];

export function OrderPage() {
  const { id } = useParams();
  const toast = useToast();
  const { data: order, error, loading, reload, setData } = useAsync(() => api<Order>(`/orders/${id}`), [id]);
  const [busy, setBusy] = useState(false);

  const changeStatus = async (status: OrderStatus) => {
    if (status === "cancelled" && !window.confirm("Bạn chắc chắn muốn huỷ đơn hàng này?")) return;
    setBusy(true);
    try {
      setData(await api<Order>(`/orders/${id}/status`, { method: "PATCH", body: { status } }));
      toast(`Đơn hàng: ${ORDER_STATUSES[status].label}`);
    } catch (err) {
      toast(errorMessage(err), "error");
      reload();
    } finally {
      setBusy(false);
    }
  };

  return (
    <Page title={order ? `Đơn hàng #${order.id}` : "Đơn hàng"} back>
      {loading && !order && <Loading />}
      {error && <ErrorState error={error} onRetry={reload} />}
      {order && (
        <>
          {order.status === "cancelled" ? (
            <div className="notice notice-muted">Đơn hàng đã bị huỷ.</div>
          ) : (
            <ol className="steps" aria-label="Tiến trình đơn hàng">
              {STEPS.map((s, i) => (
                <li key={s} className={i <= STEPS.indexOf(order.status) ? "done" : undefined}>
                  <span className="step-dot" />
                  {ORDER_STATUSES[s].label}
                </li>
              ))}
            </ol>
          )}

          <section className="card">
            <div className="section-title compact">
              <h2>{order.shopName}</h2>
              {order.shopPhone && !order.viewerIsShop && (
                <a href={`tel:${order.shopPhone}`} className="link">
                  Gọi cửa hàng
                </a>
              )}
            </div>
            <ul className="line-items">
              {order.items.map((item, i) => (
                <li key={i}>
                  <span>
                    {item.quantity}× {item.productName}
                  </span>
                  <span>{formatVnd(item.unitPrice * item.quantity)}</span>
                </li>
              ))}
            </ul>
            <div className="total-row">
              <span>Tổng cộng</span>
              <strong>{formatVnd(order.total)}</strong>
            </div>
          </section>

          <section className="card">
            <dl className="details">
              <dt>Người nhận</dt>
              <dd>{order.residentName}</dd>
              <dt>Căn hộ</dt>
              <dd>{order.deliveryApartment}</dd>
              <dt>Điện thoại</dt>
              <dd>
                <a href={`tel:${order.contactPhone}`}>{order.contactPhone}</a>
              </dd>
              <dt>Thanh toán</dt>
              <dd>{PAYMENT_METHODS[order.paymentMethod]}</dd>
              {order.note && (
                <>
                  <dt>Ghi chú</dt>
                  <dd>{order.note}</dd>
                </>
              )}
              <dt>Đặt lúc</dt>
              <dd>{formatDateTime(order.createdAt)}</dd>
            </dl>
          </section>

          {order.allowedStatuses.length > 0 && (
            <div className="actions">
              {order.allowedStatuses.map((s) => (
                <button
                  key={s}
                  className={`btn ${s === "cancelled" ? "btn-danger-outline" : "btn-primary"}`}
                  disabled={busy}
                  onClick={() => changeStatus(s)}
                >
                  {ORDER_STATUSES[s].action}
                </button>
              ))}
            </div>
          )}

          {order.canReview && <ReviewForm orderId={order.id} onDone={reload} />}
          {order.review && (
            <section className="card center">
              <p className="muted small">Đánh giá của {order.viewerIsShop ? "khách" : "bạn"}</p>
              <Stars value={order.review.rating} />
              <Link to={`/shops/${order.shopId}`} className="link">
                Xem tại trang cửa hàng
              </Link>
            </section>
          )}
        </>
      )}
    </Page>
  );
}

function ReviewForm({ orderId, onDone }: { orderId: number; onDone: () => void }) {
  const toast = useToast();
  const [rating, setRating] = useState(0);
  const [comment, setComment] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!rating) {
      setError("Vui lòng chọn số sao");
      return;
    }
    setBusy(true);
    try {
      await api(`/orders/${orderId}/review`, { method: "POST", body: { rating, comment: comment.trim() } });
      toast("Cảm ơn bạn đã đánh giá!");
      onDone();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="card form" onSubmit={submit}>
      <h2>Đánh giá đơn hàng</h2>
      <StarInput value={rating} onChange={(v) => { setRating(v); setError(null); }} />
      <Field label="Nhận xét (không bắt buộc)">
        {(id) => (
          <textarea id={id} rows={3} value={comment} onChange={(e) => setComment(e.target.value)} maxLength={1000} placeholder="Món ăn, thời gian giao, thái độ phục vụ…" />
        )}
      </Field>
      {error && <p className="form-error" role="alert">{error}</p>}
      <button className="btn btn-primary btn-block" disabled={busy}>
        {busy ? "Đang gửi…" : "Gửi đánh giá"}
      </button>
    </form>
  );
}
