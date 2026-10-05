import * as target from './global-target';

it('spies on one member', () => {
  spyOn(target, 'helper');
});
