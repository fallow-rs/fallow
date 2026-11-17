import { Node } from "./node";

export function build(nodes: Node[]): Node {
  const root = new Node();
  const stack: { parent: Node }[] = [{ parent: root }];
  for (const child of nodes) {
    const frame = stack.pop();
    if (!frame) {
      break;
    }
    const { parent } = frame;
    parent.addChild(child);
    stack.push({ parent: child });
  }
  return root;
}
