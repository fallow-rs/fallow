import { Status } from "./status";

const order = { status: Status.Paid };
console.log(order.status === Status.Shipped ? "on its way" : "waiting");
