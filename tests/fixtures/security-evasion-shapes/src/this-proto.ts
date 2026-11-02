export class AppError extends Error {
  constructor(message: string) {
    super(message);
    const actualProto = new.target.prototype;
    (this as unknown as { __proto__: object }).__proto__ = actualProto;
  }
}
