import { useQuery } from "@reduxjs/toolkit/query/react";

export { configureStore } from "@reduxjs/toolkit";

void import("@reduxjs/toolkit/query");
void import("@reduxjs/toolkit/blocked");
void import("@reduxjs/toolkit/missing");
void import("@reduxjs/toolkit/private");

console.log(useQuery);
