import { setApplication } from '@ember/test-helpers';
import { start, setupEmberOnerrorValidation } from 'ember-qunit';
import Application from 'ember-classic-fixture/app';

const config = {
  modulePrefix: 'ember-classic-fixture',
  environment: 'test',
  rootURL: '/',
};

setApplication(Application.create({ ...config, autoboot: false }));

setupEmberOnerrorValidation();
start();
