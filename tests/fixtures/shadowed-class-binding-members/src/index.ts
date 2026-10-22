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

export function sibling(): void {
  service.keptSibling();
}

export function paramShadow(service: { shadowParam(): void }): void {
  service.shadowParam();
}

export const arrowParamShadow = (service: { shadowArrowParam(): void }): void => {
  service.shadowArrowParam();
};

export function destructureShadow(): void {
  const service = { shadowDestructure(): void {} };
  const { shadowDestructure } = service;
  shadowDestructure();
}

export function paramDestructureShadow(service: { shadowParamDestructure(): void }): void {
  const { shadowParamDestructure } = service;
  shadowParamDestructure();
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
