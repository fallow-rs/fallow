export class Util {
  public property: number = 42

  public get getter() {
    return 'Hello from Util!'
  }

  public hello() {
    console.log('Hello from the function of Util!')
  }

  // Control: never accessed anywhere. Must still report as unused-class-member,
  // proving the v-for crediting does not blanket-credit the whole class.
  public deadMethod() {
    return 'never used'
  }
}
