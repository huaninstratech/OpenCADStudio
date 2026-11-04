const BASE_URL = (import.meta.env.VITE_API_URL as string | undefined) ?? "/api";
const TOKEN_KEY = "cudan.token";

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly details?: { field: string; message: string }[],
  ) {
    super(message);
  }
}

/** localStorage có thể bị chặn (chế độ riêng tư, webview) — luôn bọc try/catch. */
export const storage = {
  get(key: string): string | null {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  },
  set(key: string, value: string | null) {
    try {
      if (value === null) localStorage.removeItem(key);
      else localStorage.setItem(key, value);
    } catch {
      // bỏ qua
    }
  },
};

let token = storage.get(TOKEN_KEY);
let onUnauthorized: (() => void) | null = null;

export function setToken(next: string | null) {
  token = next;
  storage.set(TOKEN_KEY, next);
}

export function hasToken() {
  return token !== null;
}

/** AuthProvider đăng ký để tự đăng xuất khi token hết hạn. */
export function setUnauthorizedHandler(handler: (() => void) | null) {
  onUnauthorized = handler;
}

type Query = Record<string, string | number | undefined | null>;

export async function api<T>(
  path: string,
  options: { method?: string; body?: unknown; query?: Query } = {},
): Promise<T> {
  const url = new URL(BASE_URL + path, window.location.origin);
  for (const [key, value] of Object.entries(options.query ?? {})) {
    if (value !== undefined && value !== null && value !== "") url.searchParams.set(key, String(value));
  }

  let res: Response;
  try {
    res = await fetch(url, {
      method: options.method ?? "GET",
      headers: {
        ...(options.body !== undefined ? { "Content-Type": "application/json" } : {}),
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: options.body !== undefined ? JSON.stringify(options.body) : undefined,
    });
  } catch {
    throw new ApiError(0, "Không kết nối được máy chủ. Vui lòng kiểm tra mạng.");
  }

  const text = await res.text();
  let data: unknown = null;
  try {
    data = text ? JSON.parse(text) : null;
  } catch {
    data = null;
  }
  if (!res.ok) {
    const body = (data ?? {}) as { error?: string; details?: ApiError["details"] };
    if (res.status === 401 && token) onUnauthorized?.();
    throw new ApiError(res.status, body.error ?? `Lỗi ${res.status}`, body.details);
  }
  return data as T;
}

export function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : "Đã có lỗi xảy ra";
}
