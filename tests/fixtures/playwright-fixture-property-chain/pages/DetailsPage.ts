import { InputPanel } from './InputPanel';

export class DetailsPage {
  readonly input = new InputPanel();

  getParameter(name: string): string {
    return name;
  }

  unusedHelper(): number {
    return 1;
  }
}
