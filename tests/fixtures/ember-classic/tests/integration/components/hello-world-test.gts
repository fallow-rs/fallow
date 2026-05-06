import { module, test } from 'qunit';
import { setupRenderingTest } from 'ember-qunit';
import { render } from '@ember/test-helpers';
import HelloWorld from 'ember-classic-fixture/components/hello-world';

module('Integration | Component | hello-world', function (hooks) {
  setupRenderingTest(hooks);

  test('it renders a greeting', async function (assert) {
    await render(<template><HelloWorld @name="qunit" /></template>);
    assert.dom('h1').includesText('qunit');
  });
});
