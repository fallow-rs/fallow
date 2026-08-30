function guard(n) {
  if (n < 0) {
    return 0;
  }
  const scaled = n * 2;
  return scaled;
}

function clamp(n) {
  if (n > 100) {
    return 100;
  }
  const held = n + 1;
  const shifted = held * 2;
  return shifted;
}

console.log(guard(-5), clamp(500));
