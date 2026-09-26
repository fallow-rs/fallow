export function Banner() {
  return process.env.FEATURE_EMPTY_ARM ? null : <></>;
}

export function price(): number {
  if (process.env.FEATURE_SAME) {
    return 1;
  } else {
    // The same code as the guarded branch.
    return 1;
  }
}
