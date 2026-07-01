export class Util {
  public property: number = 42

  public get getter() {
    return 'Hello from Util!'
  }

  public getName() {
    return 'Util'
  }

  // Control: never accessed anywhere. Must still report as unused-class-member,
  // proving the @for / *ngFor crediting does not blanket-credit the whole class.
  public unusedMethod() {
    return 'never used'
  }
}
