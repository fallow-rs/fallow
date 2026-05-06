import Controller from "@ember/controller";
import { action } from "@ember/object";
import { inject as service } from "@ember/service";
import { tracked } from "@glimmer/tracking";
import type RouterService from "@ember/routing/router-service";

// `@ember/object`, `@ember/service`, and `@ember/routing/router-service` are
// AMD-loader / Embroider-rewritten specifiers — they are not real npm
// packages and never resolve to a file on disk. The Ember plugin's
// `virtual_module_prefixes` must keep these out of `unresolved-import` and
// `unlisted-dependency` reporting; this controller is the fixture witness.
export default class ApplicationController extends Controller {
  queryParams = ["q"];

  @service declare router: RouterService;

  @tracked q = "";

  @action
  reset() {
    this.q = "";
    this.router.transitionTo("index");
  }
}
