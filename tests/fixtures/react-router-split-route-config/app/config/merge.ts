import type { RouteConfigEntry } from "@react-router/dev/routes";

export const mergeRoutes = (core: RouteConfigEntry[], extra: RouteConfigEntry[]): RouteConfigEntry[] => [
  ...core,
  ...extra,
];
