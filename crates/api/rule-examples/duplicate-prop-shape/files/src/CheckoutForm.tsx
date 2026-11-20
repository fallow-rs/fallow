import { FieldNumber } from "./FieldNumber";
import { FieldSelect } from "./FieldSelect";
import { FieldText } from "./FieldText";

const save = (value: string): void => console.log(value);

export const CheckoutForm = () => (
  <form>
    <FieldText name="email" label="Email" value="" error="" hint="" onChange={save} />
    <FieldNumber name="quantity" label="Quantity" value="1" error="" hint="" onChange={save} />
    <FieldSelect name="country" label="Country" value="NL" error="" hint="" onChange={save} />
  </form>
);
