const { add } = require('../lib/index.js');
const { check } = require('./helper.js');

check(add(1, 2) === 3);
