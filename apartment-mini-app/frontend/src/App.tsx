import { BrowserRouter, Route, Routes } from "react-router-dom";
import { Layout } from "./components/Layout";
import { Empty, Page, RequireAuth } from "./components/ui";
import { AccountPage, LoginPage, RegisterPage } from "./pages/Account";
import { AdminPage } from "./pages/admin/Admin";
import { HomePage, PromotionsPage } from "./pages/Home";
import { AnnouncementPage, InfoPage } from "./pages/Info";
import { MerchantPage } from "./pages/merchant/Merchant";
import { CartPage, OrderPage, OrdersPage } from "./pages/Orders";
import { ShopPage, ShopsPage } from "./pages/Shops";
import { AuthProvider } from "./state/auth";
import { CartProvider } from "./state/cart";
import { ToastProvider } from "./state/toast";

export function App() {
  return (
    <BrowserRouter>
      <ToastProvider>
        <AuthProvider>
          <CartProvider>
            <Routes>
              <Route element={<Layout />}>
                <Route index element={<HomePage />} />
                <Route path="promotions" element={<PromotionsPage />} />
                <Route path="info" element={<InfoPage />} />
                <Route path="info/announcements/:id" element={<AnnouncementPage />} />
                <Route path="shops" element={<ShopsPage />} />
                <Route path="shops/:id" element={<ShopPage />} />
                <Route path="cart" element={<CartPage />} />
                <Route
                  path="orders"
                  element={
                    <RequireAuth>
                      <OrdersPage />
                    </RequireAuth>
                  }
                />
                <Route
                  path="orders/:id"
                  element={
                    <RequireAuth>
                      <OrderPage />
                    </RequireAuth>
                  }
                />
                <Route path="account" element={<AccountPage />} />
                <Route path="login" element={<LoginPage />} />
                <Route path="register" element={<RegisterPage />} />
                <Route
                  path="merchant"
                  element={
                    <RequireAuth roles={["merchant", "admin"]}>
                      <MerchantPage />
                    </RequireAuth>
                  }
                />
                <Route
                  path="admin"
                  element={
                    <RequireAuth roles={["admin"]}>
                      <AdminPage />
                    </RequireAuth>
                  }
                />
                <Route
                  path="*"
                  element={
                    <Page title="Không tìm thấy" back="/">
                      <Empty icon="🧭" title="Trang không tồn tại" />
                    </Page>
                  }
                />
              </Route>
            </Routes>
          </CartProvider>
        </AuthProvider>
      </ToastProvider>
    </BrowserRouter>
  );
}
