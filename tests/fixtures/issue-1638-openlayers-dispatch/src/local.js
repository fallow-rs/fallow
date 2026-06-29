// A LOCAL class named `PointerInteraction`, NOT the OpenLayers base. A
// dispatched-name member on a subclass of this local base must STILL report,
// proving the credit is gated on the `ol/interaction/*` import source, not on
// the base-class name alone.
export class PointerInteraction {
  baseHelper() {
    return 1;
  }
}

export class FakeInteraction extends PointerInteraction {
  handleEvent(evt) {
    return evt;
  }
}
