import { join } from "node:path";
import express, { type RequestHandler } from "express";
import { authenticate } from "./auth.js";
import type { Config } from "./config.js";
import type { AppContext } from "./context.js";
import type { DB } from "./db/index.js";
import { errorHandler, notFoundApi } from "./http.js";
import { announcementsRouter } from "./routes/announcements.js";
import { authRouter } from "./routes/auth.js";
import { buildingRouter } from "./routes/building.js";
import { ordersRouter } from "./routes/orders.js";
import { promotionsRouter } from "./routes/promotions.js";
import { reviewsRouter } from "./routes/reviews.js";
import { productsRouter, shopsRouter } from "./routes/shops.js";

function cors(origins: string[]): RequestHandler {
  return (req, res, next) => {
    const origin = req.headers.origin;
    if (origin && (origins.includes("*") || origins.includes(origin))) {
      res.setHeader("Access-Control-Allow-Origin", origin);
      res.setHeader("Vary", "Origin");
      res.setHeader("Access-Control-Allow-Headers", "Authorization, Content-Type");
      res.setHeader("Access-Control-Allow-Methods", "GET, POST, PUT, PATCH, DELETE, OPTIONS");
    }
    if (req.method === "OPTIONS") {
      res.status(204).end();
      return;
    }
    next();
  };
}

export function createApp(db: DB, config: Config) {
  const ctx: AppContext = { db, config };
  const app = express();
  app.disable("x-powered-by");

  const api = express.Router();
  if (config.corsOrigins.length) api.use(cors(config.corsOrigins));
  api.use(express.json({ limit: "100kb" }));
  api.use(authenticate(db, config.jwtSecret));

  api.get("/health", (_req, res) => {
    res.json({ ok: true });
  });
  api.use("/auth", authRouter(ctx));
  api.use("/building", buildingRouter(ctx));
  api.use("/announcements", announcementsRouter(ctx));
  api.use("/shops", shopsRouter(ctx));
  api.use("/products", productsRouter(ctx));
  api.use("/orders", ordersRouter(ctx));
  api.use("/promotions", promotionsRouter(ctx));
  api.use(reviewsRouter(ctx));
  api.use(notFoundApi);
  api.use(errorHandler);

  app.use("/api", api);

  // Phục vụ bản build của frontend (một tiến trình cho cả web + API khi triển khai).
  if (config.staticDir) {
    const indexHtml = join(config.staticDir, "index.html");
    app.use(express.static(config.staticDir, { index: false, maxAge: "1h" }));
    app.use((req, res, next) => {
      if (req.method !== "GET") return next();
      res.sendFile(indexHtml);
    });
  }

  return app;
}
