const name = process.argv[2];
const mod = await import(name);
console.log(mod);
