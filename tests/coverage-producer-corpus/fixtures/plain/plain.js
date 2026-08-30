function alpha(n) {
  if (n > 0) {
    return n;
  }
  return -n;
}

function beta(n) {
  if (n > 0) {
    return n * 2;
  }
  const doubled = n * -2;
  const shifted = doubled + 1;
  return shifted;
}

console.log(alpha(-1), beta(-1));
