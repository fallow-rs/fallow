import { route } from "@react-router/dev/routes";
import { sectionRoutes } from "./sections";

const routes = [route("admin", "./admin/page.tsx"), ...sectionRoutes];

export { routes as staffRoutes };
