import { Link } from "react-router-dom";
import { api } from "../api";
import { AnnouncementRow, PromoCard, ShopCard } from "../components/cards";
import { Icon, type IconName } from "../components/Icon";
import { ErrorState, Loading, Page, SectionTitle } from "../components/ui";
import { useAsync } from "../hooks";
import { useAuth } from "../state/auth";
import type { Announcement, Building, Promotion, Shop } from "../types";

const QUICK_LINKS: { to: string; label: string; icon: IconName }[] = [
  { to: "/info", label: "Thông báo", icon: "megaphone" },
  { to: "/info#hotline", label: "Hotline", icon: "phone" },
  { to: "/shops", label: "Đặt hàng", icon: "store" },
  { to: "/promotions", label: "Ưu đãi", icon: "tag" },
];

export function HomePage() {
  const { user } = useAuth();
  const { data, error, loading, reload } = useAsync(
    () =>
      Promise.all([
        api<Building>("/building"),
        api<Announcement[]>("/announcements", { query: { limit: 3 } }),
        api<Promotion[]>("/promotions"),
        api<Shop[]>("/shops"),
      ]),
    [],
  );

  return (
    <Page title="CưDân Hub">
      {loading && !data && <Loading />}
      {error && <ErrorState error={error} onRetry={reload} />}
      {data && (
        <>
          <section className="hero">
            <p className="hero-greeting">Xin chào{user ? `, ${user.fullName}` : ""} 👋</p>
            <h2 className="hero-title">{data[0].name}</h2>
            {user?.apartment ? (
              <span className="hero-chip">Căn hộ {user.apartment}</span>
            ) : (
              !user && (
                <Link to="/login" className="hero-chip">
                  Đăng nhập để đặt hàng →
                </Link>
              )
            )}
          </section>

          <nav className="quick-links" aria-label="Truy cập nhanh">
            {QUICK_LINKS.map((q) => (
              <Link key={q.label} to={q.to} className="quick-link">
                <span className="quick-icon">
                  <Icon name={q.icon} />
                </span>
                {q.label}
              </Link>
            ))}
          </nav>

          <SectionTitle to="/info">Thông báo mới</SectionTitle>
          <div className="stack">
            {data[1].map((a) => (
              <AnnouncementRow key={a.id} item={a} />
            ))}
          </div>

          {data[2].length > 0 && (
            <>
              <SectionTitle to="/promotions">Ưu đãi nổi bật</SectionTitle>
              <div className="h-scroll">
                {data[2].map((p) => (
                  <PromoCard key={p.id} promo={p} />
                ))}
              </div>
            </>
          )}

          <SectionTitle to="/shops">Cửa hàng trong toà nhà</SectionTitle>
          <div className="stack">
            {data[3].slice(0, 4).map((s) => (
              <ShopCard key={s.id} shop={s} />
            ))}
          </div>
        </>
      )}
    </Page>
  );
}

export function PromotionsPage() {
  const { data, error, loading, reload } = useAsync(() => api<Promotion[]>("/promotions"), []);
  return (
    <Page title="Ưu đãi & quảng bá" back>
      {loading && !data && <Loading />}
      {error && <ErrorState error={error} onRetry={reload} />}
      {data?.length === 0 && <p className="muted center">Hiện chưa có ưu đãi nào.</p>}
      <div className="stack">
        {data?.map((p) => (
          <PromoCard key={p.id} promo={p} wide />
        ))}
      </div>
    </Page>
  );
}
