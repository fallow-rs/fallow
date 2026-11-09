import { route } from "@react-router/dev/routes";
import type { RouteConfigEntry } from "@react-router/dev/routes";
import { coreRoutes } from "./config/core";
import accountRoutes from "./config/account";
import { adminRoutes as staffRoutes } from "./config";
import { mergeRoutes } from "./config/merge";

const merged: RouteConfigEntry[] = mergeRoutes(coreRoutes, staffRoutes);

export default [...merged, ...accountRoutes, route("direct", "./direct.tsx")] satisfies RouteConfigEntry[];
