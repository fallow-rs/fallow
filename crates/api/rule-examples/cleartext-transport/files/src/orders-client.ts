export const fetchOrders = async () => {
  const response = await fetch("http://api.example.com/orders");
  return response.json();
};
