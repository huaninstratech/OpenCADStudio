import { useEffect, useId, type ReactNode } from "react";
import { Link, Navigate, useLocation, useNavigate } from "react-router-dom";
import { errorMessage } from "../api";
import { SHOP_CATEGORIES, type Tone } from "../format";
import { useAuth } from "../state/auth";
import type { Role, ShopCategory } from "../types";
import { Icon } from "./Icon";

export function Page({
  title,
  back,
  action,
  children,
}: {
  title: string;
  /** `true` = quay lại trang trước; chuỗi = đường dẫn cụ thể. */
  back?: boolean | string;
  action?: ReactNode;
  children: ReactNode;
}) {
  const navigate = useNavigate();
  useEffect(() => {
    document.title = `${title} · CưDân Hub`;
  }, [title]);

  const goBack = () => {
    if (typeof back === "string") navigate(back);
    else if (window.history.state?.idx > 0) navigate(-1);
    else navigate("/");
  };

  return (
    <>
      <header className="topbar">
        {back && (
          <button className="icon-btn" onClick={goBack} aria-label="Quay lại">
            <Icon name="back" />
          </button>
        )}
        <h1>{title}</h1>
        <div className="topbar-action">{action}</div>
      </header>
      <main className="content">{children}</main>
    </>
  );
}

export function Loading({ label = "Đang tải…" }: { label?: string }) {
  return (
    <div className="state" aria-busy="true">
      <span className="spinner" />
      <span>{label}</span>
    </div>
  );
}

export function ErrorState({ error, onRetry }: { error: unknown; onRetry?: () => void }) {
  return (
    <div className="state state-error" role="alert">
      <p>{errorMessage(error)}</p>
      {onRetry && (
        <button className="btn btn-outline btn-sm" onClick={onRetry}>
          Thử lại
        </button>
      )}
    </div>
  );
}

export function Empty({ icon = "📭", title, children }: { icon?: string; title: string; children?: ReactNode }) {
  return (
    <div className="state">
      <span className="state-icon" aria-hidden="true">
        {icon}
      </span>
      <strong>{title}</strong>
      {children}
    </div>
  );
}

export function Badge({ tone = "muted", children }: { tone?: Tone; children: ReactNode }) {
  return <span className={`badge badge-${tone}`}>{children}</span>;
}

export function Stars({ value, size = "md" }: { value: number; size?: "sm" | "md" }) {
  const rounded = Math.round(value);
  return (
    <span className={`stars stars-${size}`} aria-label={`${value} trên 5 sao`}>
      {[1, 2, 3, 4, 5].map((i) => (
        <span key={i} className={i <= rounded ? "on" : undefined} aria-hidden="true">
          ★
        </span>
      ))}
    </span>
  );
}

export function StarInput({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  const labels = ["Rất tệ", "Chưa tốt", "Bình thường", "Tốt", "Tuyệt vời"];
  return (
    <div className="star-input" role="radiogroup" aria-label="Chọn số sao">
      {[1, 2, 3, 4, 5].map((i) => (
        <button
          key={i}
          type="button"
          role="radio"
          aria-checked={value === i}
          aria-label={`${i} sao – ${labels[i - 1]}`}
          className={i <= value ? "on" : undefined}
          onClick={() => onChange(i)}
        >
          ★
        </button>
      ))}
      <span className="star-input-label">{value ? labels[value - 1] : "Chạm để chọn"}</span>
    </div>
  );
}

export function ShopAvatar({ category, size = 52 }: { category: ShopCategory; size?: number }) {
  return (
    <span className={`avatar avatar-${category}`} style={{ width: size, height: size, fontSize: size * 0.48 }} aria-hidden="true">
      {SHOP_CATEGORIES[category].icon}
    </span>
  );
}

export function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: (id: string) => ReactNode;
}) {
  const id = useId();
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      {children(id)}
      {hint && <small className="hint">{hint}</small>}
    </div>
  );
}

export function Chips<T extends string>({
  options,
  value,
  onChange,
}: {
  options: { value: T; label: string }[];
  value: T;
  onChange: (v: T) => void;
}) {
  return (
    <div className="chips" role="tablist">
      {options.map((o) => (
        <button
          key={o.value}
          role="tab"
          aria-selected={value === o.value}
          className={`chip${value === o.value ? " chip-active" : ""}`}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function QtyStepper({
  value,
  onChange,
  label,
}: {
  value: number;
  onChange: (v: number) => void;
  label: string;
}) {
  return (
    <div className="stepper">
      <button type="button" onClick={() => onChange(value - 1)} aria-label={`Bớt ${label}`}>
        <Icon name="minus" size={18} />
      </button>
      <span aria-live="polite">{value}</span>
      <button type="button" onClick={() => onChange(value + 1)} aria-label={`Thêm ${label}`} disabled={value >= 99}>
        <Icon name="plus" size={18} />
      </button>
    </div>
  );
}

export function SectionTitle({ children, to, linkLabel = "Xem tất cả" }: { children: ReactNode; to?: string; linkLabel?: string }) {
  return (
    <div className="section-title">
      <h2>{children}</h2>
      {to && (
        <Link to={to} className="link">
          {linkLabel}
        </Link>
      )}
    </div>
  );
}

/** Chặn trang cần đăng nhập (và tuỳ chọn vai trò). */
export function RequireAuth({ roles, children }: { roles?: Role[]; children: ReactNode }) {
  const { user, initializing } = useAuth();
  const location = useLocation();
  if (initializing) return <Loading />;
  if (!user) return <Navigate to={`/login?next=${encodeURIComponent(location.pathname + location.search)}`} replace />;
  if (roles && !roles.includes(user.role)) {
    return (
      <Page title="Không có quyền" back>
        <Empty icon="🔒" title="Bạn không có quyền truy cập trang này" />
      </Page>
    );
  }
  return <>{children}</>;
}
