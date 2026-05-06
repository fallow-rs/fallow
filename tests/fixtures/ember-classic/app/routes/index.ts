import Route from '@ember/routing/route';

export default class IndexRoute extends Route {
  async model() {
    return { hello: 'world' };
  }
}
