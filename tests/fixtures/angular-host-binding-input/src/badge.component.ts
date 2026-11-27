import { Component, HostBinding, Input } from "@angular/core";

@Component({
  selector: "app-badge",
  template: `<span>badge</span>`,
  host: { "[class.is-large]": "large" },
})
export class BadgeComponent {
  // Angular reads each of these inputs through a host binding.
  @HostBinding("class.has-border") @Input() hasBorder = true;
  @Input() @HostBinding("class") size: "large" | "medium" = "medium";
  @Input() large = false;

  // Nothing reads this input.
  @Input() neverRead = "";
}
