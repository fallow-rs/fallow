export interface Summary {
  id: string;
}

export function summarize(): Summary {
  return { id: 'x' };
}

export interface TreeNode {
  children: TreeNode[];
}
