export class Renderer {
  public draw(): number {
    return this.paint();
  }
  protected paint(): number {
    return 0;
  }
}
