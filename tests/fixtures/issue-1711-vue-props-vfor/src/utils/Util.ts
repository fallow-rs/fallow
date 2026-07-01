export class Util {
  public id: number = 0

  public get name() {
    return 'util'
  }

  public getValue() {
    return this.id
  }

  // Control: never accessed anywhere (not in the v-for template, not elsewhere).
  // Must still report as unused-class-member, proving the props.items v-for
  // crediting does not blanket-credit the whole class.
  public unusedMethod() {
    return 'never used'
  }
}
