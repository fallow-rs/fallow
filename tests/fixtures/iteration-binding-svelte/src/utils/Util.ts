export class Util {
  public property: number = 42

  public get getter() {
    return "Hello"
  }

  public hello() {
    console.log("h")
  }

  // Control: never accessed. Must still report unused-class-member.
  public deadMethod() {
    return "never used"
  }
}
