import type { FallowApi } from "../../preload";

declare global {
  interface Window {
    fallow: FallowApi;
  }
}
