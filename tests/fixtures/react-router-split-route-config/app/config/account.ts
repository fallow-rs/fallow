import { layout, route } from "@react-router/dev/routes";

const routes = [layout("./account/layout.tsx", [route("login", "./account/login.tsx")])];

export default routes;
