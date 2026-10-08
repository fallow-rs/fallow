import { CompanyStore } from './company-store';
import { SeedCommand } from './seed.command';
import { Priority } from './priority';
import { Status } from './status';

export const service = new CompanyStore();
export const command = new SeedCommand();
export const activeStatus = Status.Active;
export const lowPriority = Priority.Low;
