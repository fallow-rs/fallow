const { a } = await import('./m.mjs');
const cond = process.argv[2] === 'x';
const backend = await import(cond ? './x.mjs' : './y.mjs');
console.log(a, backend.run());
