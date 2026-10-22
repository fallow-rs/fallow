import { Service } from './service';

const service = new Service();
service.keptModule();

export function unshadowed(): void {
  service.keptUnshadowed();
}

export function localShadow(): void {
  const service = { shadowLocal(): void {} };
  service.shadowLocal();
}

export function computedShadow(): void {
  const service = { shadowComputed(): void {} };
  service['shadowComputed']();
}

export function typeofShadow(): void {
  const service = { shadowTypeof(): void {} };
  const read = ({ shadowTypeof }: typeof service): void => shadowTypeof();
  read(service);
}

export function sibling(): void {
  service.keptSibling();
}

export function paramShadow(service: string): string {
  return service.toUpperCase();
}

export const arrowParamShadow = (service: number): string => service.toFixed();

export function destructureShadow(): void {
  const service = { shadowDestructure(): void {} };
  const { shadowDestructure } = service;
  shadowDestructure();
}

export function paramDestructureShadow(service: string): string {
  const { toLowerCase } = service;
  return toLowerCase.call(service);
}

// A type literal gives no proof: a `Service` instance satisfies it.
export function structuralParam(service: { keptStructural(): void }): void {
  service.keptStructural();
}
structuralParam(new Service());

export function structuralParamDestructure(service: { keptStructuralDestructure(): void }): void {
  const { keptStructuralDestructure } = service;
  keptStructuralDestructure();
}
structuralParamDestructure(new Service());

// `var` hoists, so the read before the declaration uses the local binding.
export function readBeforeVar(): void {
  service.shadowBeforeVar();
  var service = { other: 1 };
  void service;
}

export function closureThenSiblingShadow(): () => void {
  const read = (): void => service.keptClosure();
  {
    const service = { other: 1 };
    void service;
  }
  return read;
}

export function annotatedInstance(): void {
  const service: Service = new Service();
  service.keptAnnotated();
}

export function blockShadow(flag: boolean): void {
  if (flag) {
    const service = { shadowBlock(): void {} };
    service.shadowBlock();
  }
}

export function nestedClassBinding(): void {
  const service = new Service();
  service.keptNested();
  const { keptNestedDestructure } = service;
  keptNestedDestructure();
}

export function outer(): void {
  const service = new Service();
  function inner(): void {
    const service = { shadowInner(): void {} };
    service.shadowInner();
  }
  inner();
  service.keptOuter();
}
