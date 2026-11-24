import { slugify, truncate, capitalize, wordCount, formatDate, parseDate, addDays, isWeekend } from './utils';
import { report } from './report';

console.log(report('x'), slugify('x'), truncate('x'), capitalize('x'), wordCount('x'), formatDate('x'), parseDate('x'), addDays('x'), isWeekend('x'));
