export class ArrowReset {
  static current: unknown = null;
  createScene(): void {}
  static getInstance() {
    const reset = () => {
      ArrowReset.current = new ArrowReset();
    };
    void reset;
    return ArrowReset.current;
  }
}

export const arrowReset = ArrowReset.getInstance();
