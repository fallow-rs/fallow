const gamma = function namedGamma(n) {
  if (n > 1) {
    return n;
  }
  return 1;
};

const delta = function namedDelta(n) {
  if (n > 1) {
    return n + 1;
  }
  const lowered = n - 1;
  const shifted = lowered + 1;
  return shifted;
};

console.log(gamma(2), delta(2));
