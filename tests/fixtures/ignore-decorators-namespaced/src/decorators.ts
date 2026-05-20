function makeMethodDecorator() {
  return function (
    _target: unknown,
    _propertyKey: string,
    descriptor: PropertyDescriptor,
  ) {
    return descriptor;
  };
}

export const decorators = {
  log: makeMethodDecorator,
  audit: makeMethodDecorator,
};
