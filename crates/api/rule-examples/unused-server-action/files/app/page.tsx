import { placeOrder } from "./actions";

export default function CheckoutPage() {
  return (
    <form action={placeOrder}>
      <button type="submit">Place order</button>
    </form>
  );
}
