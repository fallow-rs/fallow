import { Aggregate } from './aggregate';

export class AggregateRepo {
    findById(_id: string): Promise<Aggregate | undefined> {
        return Promise.resolve(new Aggregate());
    }
}
