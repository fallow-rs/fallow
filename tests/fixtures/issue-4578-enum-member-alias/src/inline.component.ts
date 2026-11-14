import { Component } from '@angular/core';
import { Mood } from './enums';

@Component({ selector: 'x-inline', template: '<p>{{ moods.Happy }}</p>' })
export class InlineTemplate {
  protected readonly moods = Mood;
}
