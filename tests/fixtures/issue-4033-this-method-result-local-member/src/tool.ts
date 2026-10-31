export interface Tool {
  deactivate?(): void;
  activate?(): void;
  reset?(): void;
}

export interface Brush {
  stroke?(): void;
}
