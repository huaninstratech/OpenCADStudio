import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { DatabaseSync, type SQLInputValue } from "node:sqlite";

export type DB = DatabaseSync;
export type Params = SQLInputValue[];

const NOW = "(strftime('%Y-%m-%dT%H:%M:%fZ','now'))";

/**
 * Mỗi phần tử là một bước migration. `PRAGMA user_version` lưu số bước đã chạy,
 * nên chỉ được THÊM bước mới vào cuối — không sửa bước đã phát hành.
 */
const MIGRATIONS: string[] = [
  `
  CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    phone         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    full_name     TEXT NOT NULL,
    role          TEXT NOT NULL DEFAULT 'resident' CHECK (role IN ('resident','merchant','admin')),
    apartment     TEXT,
    created_at    TEXT NOT NULL DEFAULT ${NOW}
  );

  CREATE TABLE building_info (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    name        TEXT NOT NULL,
    address     TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    amenities   TEXT NOT NULL DEFAULT '[]'
  );

  CREATE TABLE contacts (
    id          INTEGER PRIMARY KEY,
    name        TEXT NOT NULL,
    phone       TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    sort_order  INTEGER NOT NULL DEFAULT 0
  );

  CREATE TABLE announcements (
    id         INTEGER PRIMARY KEY,
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    category   TEXT NOT NULL DEFAULT 'general'
               CHECK (category IN ('general','maintenance','event','fee','security')),
    pinned     INTEGER NOT NULL DEFAULT 0,
    author_id  INTEGER REFERENCES users(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL DEFAULT ${NOW},
    updated_at TEXT NOT NULL DEFAULT ${NOW}
  );
  CREATE INDEX idx_announcements_feed ON announcements(pinned DESC, created_at DESC);

  CREATE TABLE shops (
    id            INTEGER PRIMARY KEY,
    owner_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name          TEXT NOT NULL,
    description   TEXT NOT NULL DEFAULT '',
    category      TEXT NOT NULL CHECK (category IN ('food','drink','grocery','service','other')),
    phone         TEXT NOT NULL DEFAULT '',
    location      TEXT NOT NULL DEFAULT '',
    opening_hours TEXT NOT NULL DEFAULT '',
    image_url     TEXT,
    is_active     INTEGER NOT NULL DEFAULT 1,
    created_at    TEXT NOT NULL DEFAULT ${NOW}
  );
  CREATE INDEX idx_shops_owner ON shops(owner_id);

  CREATE TABLE products (
    id           INTEGER PRIMARY KEY,
    shop_id      INTEGER NOT NULL REFERENCES shops(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    description  TEXT NOT NULL DEFAULT '',
    price        INTEGER NOT NULL CHECK (price >= 0),
    image_url    TEXT,
    is_available INTEGER NOT NULL DEFAULT 1,
    created_at   TEXT NOT NULL DEFAULT ${NOW}
  );
  CREATE INDEX idx_products_shop ON products(shop_id);

  CREATE TABLE orders (
    id                 INTEGER PRIMARY KEY,
    resident_id        INTEGER NOT NULL REFERENCES users(id),
    shop_id            INTEGER NOT NULL REFERENCES shops(id),
    status             TEXT NOT NULL DEFAULT 'pending'
                       CHECK (status IN ('pending','confirmed','delivering','completed','cancelled')),
    delivery_apartment TEXT NOT NULL,
    contact_phone      TEXT NOT NULL,
    note               TEXT NOT NULL DEFAULT '',
    payment_method     TEXT NOT NULL DEFAULT 'cod' CHECK (payment_method IN ('cod','transfer')),
    total              INTEGER NOT NULL CHECK (total >= 0),
    created_at         TEXT NOT NULL DEFAULT ${NOW},
    updated_at         TEXT NOT NULL DEFAULT ${NOW}
  );
  CREATE INDEX idx_orders_resident ON orders(resident_id, created_at DESC);
  CREATE INDEX idx_orders_shop ON orders(shop_id, created_at DESC);

  CREATE TABLE order_items (
    id           INTEGER PRIMARY KEY,
    order_id     INTEGER NOT NULL REFERENCES orders(id) ON DELETE CASCADE,
    product_id   INTEGER REFERENCES products(id) ON DELETE SET NULL,
    product_name TEXT NOT NULL,
    unit_price   INTEGER NOT NULL CHECK (unit_price >= 0),
    quantity     INTEGER NOT NULL CHECK (quantity > 0)
  );
  CREATE INDEX idx_order_items_order ON order_items(order_id);

  CREATE TABLE promotions (
    id             INTEGER PRIMARY KEY,
    shop_id        INTEGER REFERENCES shops(id) ON DELETE CASCADE,
    created_by     INTEGER REFERENCES users(id) ON DELETE SET NULL,
    title          TEXT NOT NULL,
    content        TEXT NOT NULL DEFAULT '',
    discount_label TEXT NOT NULL DEFAULT '',
    image_url      TEXT,
    starts_at      TEXT NOT NULL,
    ends_at        TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected')),
    created_at     TEXT NOT NULL DEFAULT ${NOW},
    CHECK (ends_at > starts_at)
  );
  CREATE INDEX idx_promotions_feed ON promotions(status, ends_at);

  CREATE TABLE reviews (
    id          INTEGER PRIMARY KEY,
    shop_id     INTEGER NOT NULL REFERENCES shops(id) ON DELETE CASCADE,
    order_id    INTEGER NOT NULL UNIQUE REFERENCES orders(id) ON DELETE CASCADE,
    resident_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    rating      INTEGER NOT NULL CHECK (rating BETWEEN 1 AND 5),
    comment     TEXT NOT NULL DEFAULT '',
    reply       TEXT,
    replied_at  TEXT,
    created_at  TEXT NOT NULL DEFAULT ${NOW}
  );
  CREATE INDEX idx_reviews_shop ON reviews(shop_id, created_at DESC);
  `,
];

function migrate(db: DB): void {
  const { user_version: current } = db.prepare("PRAGMA user_version").get() as {
    user_version: number;
  };
  for (let version = current; version < MIGRATIONS.length; version++) {
    transaction(db, () => {
      db.exec(MIGRATIONS[version]);
      db.exec(`PRAGMA user_version = ${version + 1}`);
    });
  }
}

export function openDatabase(path: string): DB {
  if (path !== ":memory:") mkdirSync(dirname(path), { recursive: true });
  const db = new DatabaseSync(path);
  db.exec("PRAGMA foreign_keys = ON");
  if (path !== ":memory:") db.exec("PRAGMA journal_mode = WAL");
  migrate(db);
  return db;
}

/** Chạy `fn` trong một transaction; tự ROLLBACK nếu có lỗi. Không lồng nhau. */
export function transaction<T>(db: DB, fn: () => T): T {
  db.exec("BEGIN IMMEDIATE");
  try {
    const result = fn();
    db.exec("COMMIT");
    return result;
  } catch (err) {
    db.exec("ROLLBACK");
    throw err;
  }
}

export function nowIso(): string {
  return new Date().toISOString();
}
