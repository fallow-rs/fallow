import Helper from '@ember/component/helper';

export default class FormatDate extends Helper<{
  Args: { Positional: [Date] };
  Return: string;
}> {
  compute([date]: [Date]) {
    return date.toISOString();
  }
}
