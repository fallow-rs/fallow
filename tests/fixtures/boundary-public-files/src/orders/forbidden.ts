import { charge } from "../payments/charge";

export const forbidden = (): number => charge();
