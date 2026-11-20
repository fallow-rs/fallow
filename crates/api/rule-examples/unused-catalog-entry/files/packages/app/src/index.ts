import { z } from "zod";

export const orderId = z.string().uuid();
