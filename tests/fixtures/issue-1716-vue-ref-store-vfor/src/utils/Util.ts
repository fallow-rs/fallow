export class Util {
  public property = 'x'

  public get getter() {
    return this.property
  }

  public hello() {
    return this.getter
  }

  public unusedMethod() {
    return 'unused'
  }
}
