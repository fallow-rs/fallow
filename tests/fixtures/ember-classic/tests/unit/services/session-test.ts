import { module, test } from 'qunit';
import { setupTest } from 'ember-qunit';
import type SessionService from 'ember-classic-fixture/services/session';

module('Unit | Service | session', function (hooks) {
  setupTest(hooks);

  test('touch updates lastTouched', function (assert) {
    const service = (this as any).owner.lookup('service:session') as SessionService;
    service.touch();
    assert.ok(service.lastTouched > 0, 'lastTouched is set');
  });
});
