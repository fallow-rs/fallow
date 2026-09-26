function resolve(spec, table) {
  let hits = 0;
  for (const key of table) {
    if (key === spec) hits += 1;
  }
  return hits;
}

const lookup = (spec) => {
  const table = ['a', 'b', 'c'];
  return resolve(spec, table) + resolve(spec, table) + resolve(spec, table);
};

for (let i = 0; i < 200; i += 1) lookup('b');
