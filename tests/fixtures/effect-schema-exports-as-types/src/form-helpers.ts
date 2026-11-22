import { Schema } from "effect";

export const Text = Schema.String;

export const makeText = () => Schema.String;

export const makePlainValue = () => 1;

export const plainText = "text";
