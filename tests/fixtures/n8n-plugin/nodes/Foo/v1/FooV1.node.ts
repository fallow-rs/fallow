import type { INodeType } from 'n8n-workflow';

export class FooV1 implements INodeType {
  description = { displayName: 'Foo' };

  webhookMethods = {};

  unusedHelper(): number {
    return 1;
  }
}
