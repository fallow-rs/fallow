import { isAdmin } from "../../app/admin";

export const canRefund = (role: string): boolean => isAdmin(role);
