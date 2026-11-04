import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { storage } from "../api";
import type { Product, Shop } from "../types";

const CART_KEY = "cudan.cart";

export interface CartLine {
  product: Product;
  quantity: number;
}

/** Mỗi đơn hàng thuộc về một cửa hàng, nên giỏ hàng cũng chỉ chứa món của một cửa hàng. */
interface CartState {
  shop: Pick<Shop, "id" | "name"> | null;
  lines: CartLine[];
}

interface CartValue extends CartState {
  count: number;
  total: number;
  quantityOf(productId: number): number;
  /** Trả về `false` nếu người dùng không đồng ý xoá giỏ hàng của cửa hàng khác. */
  add(shop: Pick<Shop, "id" | "name">, product: Product): boolean;
  setQuantity(productId: number, quantity: number): void;
  clear(): void;
}

const EMPTY: CartState = { shop: null, lines: [] };
const CartContext = createContext<CartValue | null>(null);

function restore(): CartState {
  try {
    const raw = storage.get(CART_KEY);
    const parsed = raw ? (JSON.parse(raw) as CartState) : EMPTY;
    return Array.isArray(parsed.lines) ? parsed : EMPTY;
  } catch {
    return EMPTY;
  }
}

export function CartProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<CartState>(restore);

  useEffect(() => {
    storage.set(CART_KEY, state.lines.length ? JSON.stringify(state) : null);
  }, [state]);

  const value = useMemo<CartValue>(() => {
    const setQuantity = (productId: number, quantity: number) =>
      setState((s) => {
        const lines = s.lines
          .map((l) => (l.product.id === productId ? { ...l, quantity: Math.min(99, quantity) } : l))
          .filter((l) => l.quantity > 0);
        return lines.length ? { ...s, lines } : EMPTY;
      });

    return {
      ...state,
      count: state.lines.reduce((n, l) => n + l.quantity, 0),
      total: state.lines.reduce((n, l) => n + l.quantity * l.product.price, 0),
      quantityOf: (id) => state.lines.find((l) => l.product.id === id)?.quantity ?? 0,
      add(shop, product) {
        if (state.shop && state.shop.id !== shop.id && state.lines.length) {
          const ok = window.confirm(
            `Giỏ hàng đang có món của "${state.shop.name}". Xoá giỏ hàng để đặt món của "${shop.name}"?`,
          );
          if (!ok) return false;
          setState({ shop, lines: [{ product, quantity: 1 }] });
          return true;
        }
        setState((s) => {
          const existing = s.lines.find((l) => l.product.id === product.id);
          const lines = existing
            ? s.lines.map((l) => (l === existing ? { ...l, quantity: Math.min(99, l.quantity + 1) } : l))
            : [...s.lines, { product, quantity: 1 }];
          return { shop, lines };
        });
        return true;
      },
      setQuantity,
      clear: () => setState(EMPTY),
    };
  }, [state]);

  return <CartContext.Provider value={value}>{children}</CartContext.Provider>;
}

export function useCart(): CartValue {
  const ctx = useContext(CartContext);
  if (!ctx) throw new Error("useCart phải nằm trong <CartProvider>");
  return ctx;
}
