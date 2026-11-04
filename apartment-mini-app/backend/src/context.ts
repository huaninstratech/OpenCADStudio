import type { Config } from "./config.js";
import type { DB } from "./db/index.js";

export interface AppContext {
  db: DB;
  config: Config;
}
