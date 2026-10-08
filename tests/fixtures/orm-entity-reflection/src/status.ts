import { registerEnumType } from '@nestjs/graphql';

export enum Status {
  Active = 'ACTIVE',
  Closed = 'CLOSED',
}

registerEnumType(Status, { name: 'Status' });
