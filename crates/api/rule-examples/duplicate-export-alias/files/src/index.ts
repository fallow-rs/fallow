import { handler } from "./worker";
import invoiceHandler from "./worker";

console.log(handler("INV-1"), invoiceHandler("INV-2"));
