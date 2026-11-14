import { Component } from "@angular/core";

@Component({
  selector: "x-unrelated",
  template: "<p>{{ readOnlyByUnrelated }}</p>",
})
export class Unrelated {
  readOnlyByUnrelated = "";
}
