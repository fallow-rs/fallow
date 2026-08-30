const compact = (n) => { if (n > 0) { return n; } return -n; }
const idle = (n) => { if (n > 0) { return n * 2; } const held = n - 1; return held; }
console.log(compact(1), typeof idle)
