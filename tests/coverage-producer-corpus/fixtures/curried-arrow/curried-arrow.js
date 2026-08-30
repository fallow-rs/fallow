const seed = 5;

const curried = (a = seed ?? 0) =>
  (b = a ?? 0) =>
    (c = b ?? 0) => (a > c ? a + b : a - b);

const held = curried(3);
console.log(held(2)(1));
