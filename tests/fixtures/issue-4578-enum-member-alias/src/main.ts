import { Color, Hue, Shape, Size } from './enums';
import { ExternalTemplate } from './external.component';
import { InlineTemplate } from './inline.component';
import { registered } from './registry';
import { readLocal } from './shadow';

const Alias = Color;
console.log(Alias.Red);

function takesEnum(e: object): void {
  console.log(Object.keys(e).length);
}
takesEnum(Shape);

function nested(): string {
  const local = Hue;
  return local.Dark;
}

console.log({ ref: Size }, nested(), registered, ExternalTemplate, InlineTemplate, readLocal);
