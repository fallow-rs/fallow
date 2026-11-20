export interface FieldBox {
  value: string;
}

const fieldBox: FieldBox = { value: "a" };

export class FieldHolder {
  readonly latency = fieldBox;
}
