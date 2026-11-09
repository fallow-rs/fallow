import { DetailsPage } from './DetailsPage';

export class AppPage {
  readonly details: DetailsPage;

  constructor() {
    this.details = new DetailsPage();
  }

  goto(): number {
    return 1;
  }
}
