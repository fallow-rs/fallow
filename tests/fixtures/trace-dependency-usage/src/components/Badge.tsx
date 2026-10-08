import { useAppSelector } from "../store/hooks";

export const Badge = () => <b>{useAppSelector((s) => s.count)}</b>;
