import EmberRouter from '@ember/routing/router';

const config = {
  rootURL: '/',
  locationType: 'history' as const,
};

export default class Router extends EmberRouter {
  location = config.locationType;
  rootURL = config.rootURL;
}

Router.map(function (this: any) {
  this.route('about');
});
