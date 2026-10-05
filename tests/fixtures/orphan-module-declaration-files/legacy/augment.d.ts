import type { AugmentTarget } from "./augment-target";

declare global {
  interface Window {
    augment: AugmentTarget;
  }
}

export {};
