export class SingleReader {
  public async directControl(): Promise<void> {
    console.log("single direct");
  }

  public async ternarySelected(): Promise<void> {
    console.log("single ternary");
  }

  public async ifElseSelected(): Promise<void> {
    console.log("single if else");
  }

  public async switchSelected(): Promise<void> {
    console.log("single switch");
  }

  public async unusedSingle(): Promise<void> {
    console.log("unused single");
  }
}

export class MergedReader {
  public async directControl(): Promise<void> {
    console.log("merged direct");
  }

  public async ternarySelected(): Promise<void> {
    console.log("merged ternary");
  }

  public async ifElseSelected(): Promise<void> {
    console.log("merged if else");
  }

  public async switchSelected(): Promise<void> {
    console.log("merged switch");
  }

  public async ifElseDirect(): Promise<void> {
    console.log("merged direct if else");
  }

  public async switchDirect(): Promise<void> {
    console.log("merged direct switch");
  }

  public async unusedMerged(): Promise<void> {
    console.log("unused merged");
  }
}
