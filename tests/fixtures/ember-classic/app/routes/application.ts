import Route from '@ember/routing/route';
import { service } from '@ember/service';
import type SessionService from 'ember-classic-fixture/services/session';
import User from 'ember-classic-fixture/models/user';

export default class ApplicationRoute extends Route {
  @service declare session: SessionService;

  async model() {
    const user = new User();
    user.name = 'world';
    return user;
  }

  setupController(controller: any, model: any) {
    super.setupController(controller, model);
    this.session.touch();
  }
}
