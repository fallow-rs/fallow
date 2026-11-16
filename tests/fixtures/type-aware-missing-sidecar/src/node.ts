export class Node {
  readonly children: Node[] = [];

  addChild(child: Node): void {
    this.children.push(child);
  }
}
