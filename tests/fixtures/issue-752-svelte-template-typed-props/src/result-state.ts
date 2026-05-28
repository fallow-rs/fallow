// The class whose methods are consumed only through a typed `$props()` binding
// inside the Svelte component. None are instantiated with `new` in component
// code; the component receives an instance as a prop.
export class ResultState {
  pin(id: string): void {
    console.log("pin", id);
  }

  onOpen(): void {
    console.log("open");
  }

  updateLabel(label: string): void {
    console.log("label", label);
  }

  addSkipRule(rule: string): void {
    console.log("skip", rule);
  }

  // Bound via `bind:value` and read in `{#if}` in the template.
  labelInput = "";
  labelMessage = "";

  // Genuinely unused: never referenced from script or template. Must still
  // be reported so the fix does not over-credit.
  neverCalled(): void {
    console.log("dead");
  }
}
