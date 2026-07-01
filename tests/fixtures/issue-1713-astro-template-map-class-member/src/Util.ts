export class Util {
  public property: number = 42;

  public get getter() {
    return "Hello";
  }

  public hello() {
    console.log("h");
  }

  // Control: never accessed in the frontmatter or the template. Must still
  // report as unused-class-member.
  public unusedMethod() {
    return "never used";
  }
}
