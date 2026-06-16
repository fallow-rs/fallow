import { Component, Output } from "@angular/core";

// A minimal stand-in for an external event-stream source (the real-world shape
// is Angular Material's MapEventManager.getLazyEmitter, an Observable output).
class EventSource {
  lazy<T>(_name: string): { subscribe(): void } {
    return { subscribe() {} };
  }
}

@Component({
  selector: "app-observable-output",
  template: `<div>observable-output</div>`,
})
export class ObservableOutputComponent {
  private source = new EventSource();

  // Typed as an external stream, initialized by a lazy-emitter call rather than
  // `new EventEmitter()`. It emits without `this.streamed.emit()`, so it must NOT
  // be harvested as a dead-output candidate (zero-FP narrowing).
  @Output() readonly streamed = this.source.lazy<void>("changed");
}
