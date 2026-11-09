import { DetailsPage } from './DetailsPage';

export class AppPage {
  readonly details: DetailsPage = new DetailsPage();

  goto(): number {
    return 1;
  }
}
