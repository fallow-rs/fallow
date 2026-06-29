import PointerInteraction from "ol/interaction/Pointer";

// `handleEvent` (and the `handle*Event` / `stopDown` protocol) is invoked by
// OpenLayers by convention, never through an explicit `instance.method()` call.
// It must be credited as runtime-used. `trulyUnused` is a genuine dead member
// on the SAME class and must keep reporting (the non-vacuous control).
export class DragInteraction extends PointerInteraction {
  handleEvent(evt) {
    return super.handleEvent(evt);
  }

  handleDragEvent(evt) {
    return evt;
  }

  trulyUnused() {
    return 42;
  }
}
