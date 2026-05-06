import Model, { attr } from '@ember-data/model';

export default class User extends Model {
  @attr('string') declare name: string;
  @attr('string') declare email: string;
}
