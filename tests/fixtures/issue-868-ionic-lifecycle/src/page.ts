import type { OnInit } from "@angular/core";
import type {
  ViewDidEnter,
  ViewDidLeave,
  ViewWillEnter,
  ViewWillLeave,
} from "@ionic/angular";

export class IonicPage
  implements OnInit, ViewWillEnter, ViewDidEnter, ViewWillLeave, ViewDidLeave
{
  ngOnInit(): void {
    this.load();
  }

  ionViewWillEnter(): void {
    this.reload();
  }

  ionViewDidEnter(): void {
    this.trackReady();
  }

  ionViewWillLeave(): void {
    this.cleanup();
  }

  ionViewDidLeave(): void {
    this.afterLeave();
  }

  unusedHelper(): void {}

  private load(): void {}

  private reload(): void {}

  private trackReady(): void {}

  private cleanup(): void {}

  private afterLeave(): void {}
}

export class PlainClass {
  ionViewWillEnter(): void {}

  ionViewWillLoad(): void {}

  unusedHelper(): void {}
}
