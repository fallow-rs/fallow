const Types = require('./req-types');

export const useRequireTypes = (value: Types.ReqA): Types.ReqB => String(value.value);
