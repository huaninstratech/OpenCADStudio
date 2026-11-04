import { useState, type FormEvent } from "react";
import { Link } from "react-router-dom";
import { api, errorMessage } from "../../api";
import { PromotionForm } from "../../components/PromotionForm";
import { Chips, Empty, ErrorState, Field, Loading, Page } from "../../components/ui";
import { ANNOUNCEMENT_CATEGORIES, timeAgo } from "../../format";
import { useAsync } from "../../hooks";
import { useToast } from "../../state/toast";
import type { Announcement, AnnouncementCategory, Promotion } from "../../types";
import { PromotionList } from "../merchant/Merchant";

type Tab = "announcements" | "promotions";

export function AdminPage() {
  const [tab, setTab] = useState<Tab>("announcements");
  return (
    <Page title="Quản trị BQL" back="/account">
      <Chips
        options={[
          { value: "announcements", label: "Thông báo" },
          { value: "promotions", label: "Duyệt quảng bá" },
        ]}
        value={tab}
        onChange={setTab}
      />
      {tab === "announcements" ? <Announcements /> : <Promotions />}
    </Page>
  );
}

function Announcements() {
  const toast = useToast();
  const { data, error, loading, reload } = useAsync(() => api<Announcement[]>("/announcements", { query: { limit: 100 } }), []);
  const [form, setForm] = useState({ title: "", content: "", category: "general" as AnnouncementCategory, pinned: false });
  const [busy, setBusy] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setFormError(null);
    try {
      await api("/announcements", { method: "POST", body: form });
      setForm({ title: "", content: "", category: "general", pinned: false });
      toast("Đã đăng thông báo");
      reload();
    } catch (err) {
      setFormError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const togglePin = async (a: Announcement) => {
    try {
      await api(`/announcements/${a.id}`, { method: "PATCH", body: { pinned: !a.pinned } });
      reload();
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };

  const remove = async (a: Announcement) => {
    if (!window.confirm(`Xoá thông báo "${a.title}"?`)) return;
    try {
      await api(`/announcements/${a.id}`, { method: "DELETE" });
      toast("Đã xoá thông báo");
      reload();
    } catch (err) {
      toast(errorMessage(err), "error");
    }
  };

  return (
    <>
      <form className="card form" onSubmit={submit}>
        <h2>Đăng thông báo mới</h2>
        <Field label="Tiêu đề">
          {(id) => <input id={id} required maxLength={200} value={form.title} onChange={(e) => setForm({ ...form, title: e.target.value })} />}
        </Field>
        <Field label="Danh mục">
          {(id) => (
            <select id={id} value={form.category} onChange={(e) => setForm({ ...form, category: e.target.value as AnnouncementCategory })}>
              {Object.entries(ANNOUNCEMENT_CATEGORIES).map(([value, c]) => (
                <option key={value} value={value}>
                  {c.icon} {c.label}
                </option>
              ))}
            </select>
          )}
        </Field>
        <Field label="Nội dung">
          {(id) => (
            <textarea id={id} required rows={4} maxLength={10000} value={form.content} onChange={(e) => setForm({ ...form, content: e.target.value })} />
          )}
        </Field>
        <label className="switch">
          <input type="checkbox" checked={form.pinned} onChange={(e) => setForm({ ...form, pinned: e.target.checked })} />
          <span>Ghim lên đầu</span>
        </label>
        {formError && <p className="form-error" role="alert">{formError}</p>}
        <button className="btn btn-primary btn-block" disabled={busy}>
          {busy ? "Đang đăng…" : "Đăng thông báo"}
        </button>
      </form>

      {loading && !data && <Loading />}
      {error && <ErrorState error={error} onRetry={reload} />}
      {data?.length === 0 && <Empty title="Chưa có thông báo" />}
      <div className="stack">
        {data?.map((a) => (
          <article key={a.id} className="card">
            <div className="row-between">
              <Link to={`/info/announcements/${a.id}`}>
                <strong>{a.title}</strong>
              </Link>
            </div>
            <p className="muted small">
              {ANNOUNCEMENT_CATEGORIES[a.category].label} · {timeAgo(a.createdAt)}
              {a.pinned && " · Đang ghim"}
            </p>
            <div className="row-actions">
              <button className="btn btn-outline btn-sm" onClick={() => togglePin(a)}>
                {a.pinned ? "Bỏ ghim" : "Ghim"}
              </button>
              <button className="btn btn-ghost btn-sm" onClick={() => remove(a)}>
                Xoá
              </button>
            </div>
          </article>
        ))}
      </div>
    </>
  );
}

function Promotions() {
  const toast = useToast();
  const { data, error, loading, reload } = useAsync(() => api<Promotion[]>("/promotions/manage"), []);
  const pending = data?.filter((p) => p.status === "pending").length ?? 0;
  return (
    <>
      {loading && !data && <Loading />}
      {error && <ErrorState error={error} onRetry={reload} />}
      {data && (
        <>
          <p className="muted small">{pending ? `${pending} tin đang chờ duyệt` : "Không có tin nào chờ duyệt"}</p>
          <PromotionList promos={data} onChanged={reload} moderate />
        </>
      )}
      <PromotionForm
        shopId={null}
        note="Tin của Ban quản lý được đăng ngay, không cần duyệt."
        onCreated={() => {
          toast("Đã đăng tin");
          reload();
        }}
      />
    </>
  );
}
