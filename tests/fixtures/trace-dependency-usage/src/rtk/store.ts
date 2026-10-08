import { configureStore, createSlice } from "@reduxjs/toolkit";

export const slice = createSlice({ name: "counter", initialState: 0, reducers: { inc: (n: number) => n + 1 } });
export const store = configureStore({ reducer: slice.reducer });
