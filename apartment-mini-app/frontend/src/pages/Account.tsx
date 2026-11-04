import { useState, type FormEvent } from "react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";
import { errorMessage } from "../api";
import { Icon } from "../components/Icon";
import { Badge, Field, Page } from "../components/ui";
import { ROLES } from "../format";
import { useAuth } from "../state/auth";
import { useToast } from "../state/toast";

const SHOW_DEMO = import.meta.env.VITE_DEMO !== "false";
const DEMO = [
  { label: "Cư dân", phone: "0900000010", password: "cudan123" },
  { label: "Chủ cửa hàng", phone: "0900000002", password: "shop123" },
  { label: "Ban quản lý", phone: "0900000001", password: "admin123" },
];

/** Chỉ cho phép chuyển hướng nội bộ để tránh open redirect qua ?next=. */
function safeNext(value: string | null) {
  return value && value.startsWith("/") && !value.startsWith("//") ? value : "/";
}

export function LoginPage() {
  const { login } = useAuth();
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const [phone, setPhone] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const next = safeNext(params.get("next"));

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await login(phone.trim(), password);
      navigate(next, { replace: true });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Page title="Đăng nhập" back>
      <form className="card form" onSubmit={submit}>
        <Field label="Số điện thoại">
          {(id) => <input id={id} type="tel" autoComplete="tel" required value={phone} onChange={(e) => setPhone(e.target.value)} />}
        </Field>
        <Field label="Mật khẩu">
          {(id) => (
            <input id={id} type="password" autoComplete="current-password" required value={password} onChange={(e) => setPassword(e.target.value)} />
          )}
        </Field>
        {error && <p className="form-error" role="alert">{error}</p>}
        <button className="btn btn-primary btn-block" disabled={busy}>
          {busy ? "Đang đăng nhập…" : "Đăng nhập"}
        </button>
        <p className="center small">
          Chưa có tài khoản? <Link to={`/register?next=${encodeURIComponent(next)}`} className="link">Đăng ký</Link>
        </p>
      </form>

      {SHOW_DEMO && (
        <section className="card demo">
          <h2>Tài khoản demo</h2>
          <p className="muted small">Chạm để điền sẵn thông tin đăng nhập.</p>
          <div className="demo-list">
            {DEMO.map((d) => (
              <button
                key={d.phone}
                type="button"
                className="btn btn-outline btn-sm"
                onClick={() => {
                  setPhone(d.phone);
                  setPassword(d.password);
                }}
              >
                {d.label}
              </button>
            ))}
          </div>
        </section>
      )}
    </Page>
  );
}

export function RegisterPage() {
  const { register } = useAuth();
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const [form, setForm] = useState({
    fullName: "",
    phone: "",
    apartment: "",
    password: "",
    role: "resident" as "resident" | "merchant",
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
      const user = await register({ ...form, apartment: form.apartment.trim() || undefined });
      navigate(user.role === "merchant" ? "/merchant" : safeNext(params.get("next")), { replace: true });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Page title="Đăng ký" back>
      <form className="card form" onSubmit={submit}>
        <fieldset className="radio-group radio-row">
          <legend>Bạn là</legend>
          {(["resident", "merchant"] as const).map((r) => (
            <label key={r} className={`radio${form.role === r ? " radio-active" : ""}`}>
              <input type="radio" name="role" value={r} checked={form.role === r} onChange={set("role")} />
              {ROLES[r]}
            </label>
          ))}
        </fieldset>
        <Field label="Họ và tên">
          {(id) => <input id={id} required autoComplete="name" value={form.fullName} onChange={set("fullName")} maxLength={100} />}
        </Field>
        <Field label="Số điện thoại">
          {(id) => <input id={id} type="tel" required autoComplete="tel" value={form.phone} onChange={set("phone")} />}
        </Field>
        <Field label="Căn hộ" hint="VD: A-12.05 — dùng làm địa chỉ nhận hàng mặc định">
          {(id) => <input id={id} value={form.apartment} onChange={set("apartment")} maxLength={30} />}
        </Field>
        <Field label="Mật khẩu" hint="Tối thiểu 6 ký tự">
          {(id) => (
            <input id={id} type="password" required minLength={6} autoComplete="new-password" value={form.password} onChange={set("password")} />
          )}
        </Field>
        {error && <p className="form-error" role="alert">{error}</p>}
        <button className="btn btn-primary btn-block" disabled={busy}>
          {busy ? "Đang tạo tài khoản…" : "Tạo tài khoản"}
        </button>
      </form>
    </Page>
  );
}

export function AccountPage() {
  const { user, logout, updateProfile, initializing } = useAuth();
  const toast = useToast();
  const [editing, setEditing] = useState(false);
  const [fullName, setFullName] = useState("");
  const [apartment, setApartment] = useState("");
  const [error, setError] = useState<string | null>(null);

  if (!user) {
    return (
      <Page title="Tài khoản">
        <section className="card center form">
          <span className="state-icon" aria-hidden="true">🏠</span>
          <h2>Chào mừng đến CưDân Hub</h2>
          <p className="muted">Đăng nhập để đặt hàng, theo dõi đơn và đánh giá cửa hàng.</p>
          <Link to="/login" className="btn btn-primary btn-block" aria-disabled={initializing}>
            Đăng nhập
          </Link>
          <Link to="/register" className="btn btn-outline btn-block">
            Tạo tài khoản mới
          </Link>
        </section>
      </Page>
    );
  }

  const startEdit = () => {
    setFullName(user.fullName);
    setApartment(user.apartment ?? "");
    setError(null);
    setEditing(true);
  };

  const save = async (e: FormEvent) => {
    e.preventDefault();
    try {
      await updateProfile({ fullName: fullName.trim(), apartment: apartment.trim() || null });
      setEditing(false);
      toast("Đã cập nhật hồ sơ");
    } catch (err) {
      setError(errorMessage(err));
    }
  };

  return (
    <Page title="Tài khoản">
      <section className="card profile">
        <span className="profile-avatar" aria-hidden="true">
          {user.fullName.trim().split(/\s+/).pop()?.[0]?.toUpperCase()}
        </span>
        <div>
          <strong>{user.fullName}</strong>
          <span className="muted">{user.phone}</span>
          <div className="profile-tags">
            <Badge tone="info">{ROLES[user.role]}</Badge>
            {user.apartment && <Badge>Căn {user.apartment}</Badge>}
          </div>
        </div>
      </section>

      {editing ? (
        <form className="card form" onSubmit={save}>
          <Field label="Họ và tên">
            {(id) => <input id={id} required value={fullName} onChange={(e) => setFullName(e.target.value)} maxLength={100} />}
          </Field>
          <Field label="Căn hộ">
            {(id) => <input id={id} value={apartment} onChange={(e) => setApartment(e.target.value)} maxLength={30} />}
          </Field>
          {error && <p className="form-error" role="alert">{error}</p>}
          <div className="actions">
            <button type="button" className="btn btn-outline" onClick={() => setEditing(false)}>
              Huỷ
            </button>
            <button className="btn btn-primary">Lưu</button>
          </div>
        </form>
      ) : (
        <nav className="card list" aria-label="Tài khoản">
          <button className="list-row" onClick={startEdit}>
            <span>Chỉnh sửa hồ sơ</span>
            <Icon name="chevron" size={18} />
          </button>
          <Link to="/orders" className="list-row">
            <span>Đơn hàng của tôi</span>
            <Icon name="chevron" size={18} />
          </Link>
          {(user.role === "merchant" || user.role === "admin") && (
            <Link to="/merchant" className="list-row">
              <span>
                <Icon name="store" size={18} /> Quản lý cửa hàng
              </span>
              <Icon name="chevron" size={18} />
            </Link>
          )}
          {user.role === "admin" && (
            <Link to="/admin" className="list-row">
              <span>
                <Icon name="shield" size={18} /> Quản trị Ban quản lý
              </span>
              <Icon name="chevron" size={18} />
            </Link>
          )}
          <button className="list-row danger" onClick={logout}>
            <span>
              <Icon name="logout" size={18} /> Đăng xuất
            </span>
          </button>
        </nav>
      )}
    </Page>
  );
}
