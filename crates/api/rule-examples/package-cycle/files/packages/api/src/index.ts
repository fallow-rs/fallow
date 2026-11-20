import { findOrder } from "@acme/db";

export type Order = { id: string; total: number };

export const getOrder = (id: string) => findOrder(id);
