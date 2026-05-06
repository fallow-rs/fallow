import Modifier from 'ember-modifier';

export default class AutofocusModifier extends Modifier<{
  Element: HTMLElement;
}> {
  modify(element: HTMLElement) {
    element.focus();
  }
}
