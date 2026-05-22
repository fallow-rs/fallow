export class NamedBuilder {
  notNull() {
    return this;
  }

  default(value: unknown) {
    void value;
    return this;
  }
}
