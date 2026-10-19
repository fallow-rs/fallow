import { createClient } from "@libsql/client";
import { drizzle } from "drizzle-orm/libsql";
import { integer, sqliteTable } from "drizzle-orm/sqlite-core";

export const records = sqliteTable("records", { value: integer("value") });

export const db = drizzle(createClient({ url: "file:local.db" }));
