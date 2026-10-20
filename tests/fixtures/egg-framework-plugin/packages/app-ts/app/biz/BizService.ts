import { SingletonProto } from '@eggjs/tegg';

@SingletonProto()
export class BizService {
  hello(): string {
    return 'hello';
  }
}
