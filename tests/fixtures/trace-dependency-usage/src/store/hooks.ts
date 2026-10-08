import { useDispatch, useSelector } from "react-redux";
import type { AppDispatch, RootState } from "./types";

export const useAppSelector = useSelector.withTypes<RootState>();
export const useAppDispatch: () => AppDispatch = useDispatch;
