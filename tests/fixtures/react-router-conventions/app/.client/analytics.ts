import { track } from "browser-analytics";

export function trackRouteView(route: string) {
  track(route);
}
