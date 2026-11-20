import type { Order } from "@acme/api";

export const findOrder = (id: string): Order => ({ id, total: 0 });
