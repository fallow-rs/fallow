import { useAppSelector } from "../store/barrel";

export const Total = () => useAppSelector((s) => s.count);
