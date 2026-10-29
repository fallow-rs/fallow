export interface Tool {
  onDown?(): void;
  onUp?(): void;
}

export interface Brush {
  stroke?(): void;
}

export interface Shape {
  fill?(): void;
}
