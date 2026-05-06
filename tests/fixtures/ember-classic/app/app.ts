import Application from '@ember/application';
import Resolver from 'ember-resolver';
import loadInitializers from 'ember-load-initializers';
import Router from './router';

const config = {
  modulePrefix: 'ember-classic-fixture',
  environment: 'development',
  rootURL: '/',
  locationType: 'history',
};

export default class App extends Application {
  modulePrefix = config.modulePrefix;
  podModulePrefix = undefined;
  Resolver = Resolver;
}

(App.prototype as any).Router = Router;

loadInitializers(App as any, config.modulePrefix);
