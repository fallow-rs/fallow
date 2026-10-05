import * as target from './jest-target';

test('spies on one member', () => {
  jest.spyOn(target, `helper`);
});
