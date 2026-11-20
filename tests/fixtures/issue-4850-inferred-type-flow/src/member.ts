export interface MemberBox {
  value: string;
}

// `holder.box` reads one property, so the type of `other` does not reach
// `memberStatus`.
export interface MemberOther {
  flag: boolean;
}

export interface Unrelated {
  note: string;
}

const memberBox: MemberBox = { value: "a" };

const other: MemberOther = { flag: true };

const holder = { box: memberBox, other };

export const memberStatus = { latency: holder.box };
