import { Day } from './day';

export class Month {
  days: Day[] = [];

  first(): number | undefined {
    return this.days[0]?.viaThisIndex();
  }

  get sorted(): readonly Day[] {
    return this.days;
  }
}
