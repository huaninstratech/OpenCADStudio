import { NavLink, Outlet } from "react-router-dom";
import { Icon, type IconName } from "./Icon";

const TABS: { to: string; label: string; icon: IconName }[] = [
  { to: "/", label: "Trang chủ", icon: "home" },
  { to: "/info", label: "Thông tin", icon: "building" },
  { to: "/shops", label: "Cửa hàng", icon: "store" },
  { to: "/orders", label: "Đơn hàng", icon: "receipt" },
  { to: "/account", label: "Tài khoản", icon: "user" },
];

export function Layout() {
  return (
    <div className="app">
      <Outlet />
      <nav className="tabbar" aria-label="Điều hướng chính">
        {TABS.map((t) => (
          <NavLink key={t.to} to={t.to} end={t.to === "/"} className="tab">
            <Icon name={t.icon} />
            <span>{t.label}</span>
          </NavLink>
        ))}
      </nav>
    </div>
  );
}
