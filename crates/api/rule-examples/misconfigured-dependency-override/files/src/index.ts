import axios from "axios";

export const fetchOrders = () => axios.get("/api/orders");
