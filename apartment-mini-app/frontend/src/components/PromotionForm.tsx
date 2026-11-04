import { useState, type FormEvent } from "react";
import { api, errorMessage } from "../api";
import { toDateInput } from "../format";
import type { Promotion } from "../types";
import { Field } from "./ui";

/** Form tạo tin quảng bá. `shopId = null` nghĩa là tin của BQL. */
export function PromotionForm({
  shopId,
  note,
  onCreated,
}: {
  shopId: number | null;
  note?: string;
  onCreated: (promo: Promotion) => void;
}) {
  const today = new Date();
  const [form, setForm] = useState({
    title: "",
    discountLabel: "",
    content: "",
    startsAt: toDateInput(today),
    endsAt: toDateInput(new Date(today.getTime() + 14 * 86_400_000)),
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
      const promo = await api<Promotion>("/promotions", {
        method: "POST",
        body: {
          shopId,
          title: form.title,
          discountLabel: form.discountLabel,
          content: form.content,
          // Ngày theo giờ địa phương: bắt đầu từ 0h, kết thúc hết ngày.
          startsAt: new Date(`${form.startsAt}T00:00:00`).toISOString(),
          endsAt: new Date(`${form.endsAt}T23:59:59`).toISOString(),
        },
      });
      setForm((f) => ({ ...f, title: "", discountLabel: "", content: "" }));
      onCreated(promo);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form className="card form" onSubmit={submit}>
      <h2>Tạo tin quảng bá</h2>
      {note && <p className="muted small">{note}</p>}
      <Field label="Tiêu đề">
        {(id) => <input id={id} required maxLength={150} value={form.title} onChange={set("title")} placeholder="VD: Mua 2 tặng 1 cuối tuần" />}
      </Field>
      <Field label="Nhãn ưu đãi" hint="Hiển thị nổi bật, VD: -20%, Freeship">
        {(id) => <input id={id} maxLength={50} value={form.discountLabel} onChange={set("discountLabel")} />}
      </Field>
      <Field label="Nội dung">
        {(id) => <textarea id={id} rows={3} maxLength={2000} value={form.content} onChange={set("content")} />}
      </Field>
      <div className="grid-2">
        <Field label="Từ ngày">
          {(id) => <input id={id} type="date" required value={form.startsAt} onChange={set("startsAt")} />}
        </Field>
        <Field label="Đến ngày">
          {(id) => <input id={id} type="date" required min={form.startsAt} value={form.endsAt} onChange={set("endsAt")} />}
        </Field>
      </div>
      {error && <p className="form-error" role="alert">{error}</p>}
      <button className="btn btn-primary btn-block" disabled={busy}>
        {busy ? "Đang gửi…" : "Đăng tin"}
      </button>
    </form>
  );
}
