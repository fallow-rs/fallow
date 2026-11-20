import leftPad from "left-pad";

export const orderNumber = (id: number): string => leftPad(String(id), 8, "0");
