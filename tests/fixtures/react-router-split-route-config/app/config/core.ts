import { index, route } from "@react-router/dev/routes";
import type { RouteConfigEntry } from "@react-router/dev/routes";

export const coreRoutes: RouteConfigEntry[] = [
  index("./marketing/home.tsx"),
  route("about", "./marketing/about.tsx"),
  route("items/:id", "./(app)/items/[id]/page.tsx"),
];
