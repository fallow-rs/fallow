import { Button } from "./Button";

const ButtonWrapper = (props: { label: string }) => <Button {...props} />;

export const CheckoutForm = () => (
  <form>
    <ButtonWrapper label="Place order" />
  </form>
);
