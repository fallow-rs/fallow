import { Component } from '@angular/core';
import { Kind, Level, Tone } from './enums';

@Component({ selector: 'x-external', templateUrl: './external.component.html' })
export class ExternalTemplate {
  protected readonly Tone = Tone;
  protected readonly levels = Level;
  protected readonly Kind = Kind;

  isTwo(): boolean {
    return this.Kind.Two === 'two';
  }
}
