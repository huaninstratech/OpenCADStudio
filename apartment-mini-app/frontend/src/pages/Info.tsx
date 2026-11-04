import { useEffect, useState } from "react";
import { useLocation, useParams } from "react-router-dom";
import { api } from "../api";
import { AnnouncementRow } from "../components/cards";
import { Icon } from "../components/Icon";
import { Chips, Empty, ErrorState, Loading, Page, SectionTitle } from "../components/ui";
import { ANNOUNCEMENT_CATEGORIES, formatDateTime } from "../format";
import { useAsync } from "../hooks";
import type { Announcement, AnnouncementCategory, Building } from "../types";

type Filter = AnnouncementCategory | "all";
const FILTERS: { value: Filter; label: string }[] = [
  { value: "all", label: "Tất cả" },
  ...Object.entries(ANNOUNCEMENT_CATEGORIES).map(([value, c]) => ({ value: value as Filter, label: c.label })),
];

export function InfoPage() {
  const [filter, setFilter] = useState<Filter>("all");
  const location = useLocation();
  const building = useAsync(() => api<Building>("/building"), []);
  const announcements = useAsync(
    () => api<Announcement[]>("/announcements", { query: { category: filter === "all" ? undefined : filter } }),
    [filter],
  );

  useEffect(() => {
    if (location.hash && building.data) {
      document.getElementById(location.hash.slice(1))?.scrollIntoView({ behavior: "smooth" });
    }
  }, [location.hash, building.data]);

  return (
    <Page title="Thông tin chung cư">
      {building.loading && !building.data && <Loading />}
      {building.error && <ErrorState error={building.error} onRetry={building.reload} />}
      {building.data && (
        <>
          <section className="card building-card">
            <h2>{building.data.name}</h2>
            {building.data.address && (
              <p className="meta">
                <Icon name="location" size={15} /> {building.data.address}
              </p>
            )}
            {building.data.description && <p>{building.data.description}</p>}
            {building.data.amenities.length > 0 && (
              <ul className="tags" aria-label="Tiện ích">
                {building.data.amenities.map((a) => (
                  <li key={a}>{a}</li>
                ))}
              </ul>
            )}
          </section>

          <SectionTitle>
            <span id="hotline">Hotline</span>
          </SectionTitle>
          <div className="card list">
            {building.data.contacts.map((c) => (
              <a key={c.id} href={`tel:${c.phone}`} className="list-row">
                <div>
                  <strong>{c.name}</strong>
                  <span className="muted">{c.description}</span>
                </div>
                <span className="call-btn">
                  <Icon name="phone" size={18} /> {c.phone}
                </span>
              </a>
            ))}
          </div>
        </>
      )}

      <SectionTitle>Thông báo từ Ban quản lý</SectionTitle>
      <Chips options={FILTERS} value={filter} onChange={setFilter} />
      {announcements.loading && !announcements.data && <Loading />}
      {announcements.error && <ErrorState error={announcements.error} onRetry={announcements.reload} />}
      {announcements.data?.length === 0 && <Empty title="Chưa có thông báo nào" />}
      <div className="stack">
        {announcements.data?.map((a) => (
          <AnnouncementRow key={a.id} item={a} />
        ))}
      </div>
    </Page>
  );
}

export function AnnouncementPage() {
  const { id } = useParams();
  const { data, error, loading, reload } = useAsync(() => api<Announcement>(`/announcements/${id}`), [id]);
  return (
    <Page title="Thông báo" back="/info">
      {loading && !data && <Loading />}
      {error && <ErrorState error={error} onRetry={reload} />}
      {data && (
        <article className="card article">
          <span className="muted">
            {ANNOUNCEMENT_CATEGORIES[data.category].icon} {ANNOUNCEMENT_CATEGORIES[data.category].label}
          </span>
          <h2>{data.title}</h2>
          <p className="muted small">
            {data.authorName ?? "Ban quản lý"} · {formatDateTime(data.createdAt)}
          </p>
          <div className="article-body">{data.content}</div>
        </article>
      )}
    </Page>
  );
}
