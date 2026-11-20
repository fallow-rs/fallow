import { captureRegion } from "./capture.js";

export default class CaptureExtension {
  enable() {
    captureRegion();
  }

  disable() {}
}
