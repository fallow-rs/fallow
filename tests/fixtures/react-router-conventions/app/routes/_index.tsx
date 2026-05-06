import { trackRouteView } from "../.client/analytics";
import { db } from "../.server/db";

void db;
trackRouteView("index");

export async function loader() {
  return null;
}

export async function clientLoader() {
  return null;
}

export async function action() {
  return null;
}

export async function clientAction() {
  return null;
}

export function meta() {
  return [];
}

export function links() {
  return [];
}

export function headers() {
  return {};
}

export function ErrorBoundary() {
  return null;
}

export function HydrateFallback() {
  return null;
}

export function shouldRevalidate() {
  return true;
}

export function middleware() {
  return null;
}

export function clientMiddleware() {
  return null;
}

export const handle = { route: "index" };
export const unusedRouteHelper = () => null;

export default function IndexRoute() {
  return null;
}
