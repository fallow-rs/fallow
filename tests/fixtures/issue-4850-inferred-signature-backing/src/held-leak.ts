interface Hidden {
  a: number;
}

const hidden: Hidden = { a: 1 };

export const heldLeak = { hidden };
