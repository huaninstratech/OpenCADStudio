import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { api, hasToken, setToken, setUnauthorizedHandler } from "../api";
import type { Role, Session, User } from "../types";

interface RegisterInput {
  phone: string;
  password: string;
  fullName: string;
  apartment?: string;
  role: Exclude<Role, "admin">;
}

interface AuthValue {
  user: User | null;
  /** `true` trong lúc kiểm tra token đã lưu khi mở app. */
  initializing: boolean;
  login(phone: string, password: string): Promise<User>;
  register(input: RegisterInput): Promise<User>;
  logout(): void;
  updateProfile(patch: { fullName?: string; apartment?: string | null }): Promise<User>;
}

const AuthContext = createContext<AuthValue | null>(null);

export function AuthProvider({ children }: { children: ReactNode }) {
  const [user, setUser] = useState<User | null>(null);
  const [initializing, setInitializing] = useState(hasToken());

  const logout = useCallback(() => {
    setToken(null);
    setUser(null);
  }, []);

  useEffect(() => {
    setUnauthorizedHandler(logout);
    if (hasToken()) {
      api<User>("/auth/me")
        .then(setUser, () => logout())
        .finally(() => setInitializing(false));
    }
    return () => setUnauthorizedHandler(null);
  }, [logout]);

  const value = useMemo<AuthValue>(() => {
    const start = (session: Session) => {
      setToken(session.token);
      setUser(session.user);
      return session.user;
    };
    return {
      user,
      initializing,
      logout,
      login: async (phone, password) =>
        start(await api<Session>("/auth/login", { method: "POST", body: { phone, password } })),
      register: async (input) =>
        start(await api<Session>("/auth/register", { method: "POST", body: input })),
      updateProfile: async (patch) => {
        const next = await api<User>("/auth/me", { method: "PATCH", body: patch });
        setUser(next);
        return next;
      },
    };
  }, [user, initializing, logout]);

  return <AuthContext.Provider value={value}>{children}</AuthContext.Provider>;
}

export function useAuth(): AuthValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error("useAuth phải nằm trong <AuthProvider>");
  return ctx;
}
