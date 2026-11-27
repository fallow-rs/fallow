import { newService } from "./context";

const setup = () => newService("shadow");

const makeOther = () => ({ ctx: { shadowWrapperOnly: (): void => {} } });

export const shadowed = (): void => {
  const setup = makeOther;
  const { ctx: other } = setup();
  other.shadowWrapperOnly();
};

let reset = () => newService("reset");
reset = () => ({ ctx: new TestDouble() }) as unknown as ReturnType<typeof newService>;

class TestDouble {
  reassignedWrapperOnly(): void {}
}

export const reassigned = (): void => {
  const { ctx: replaced } = reset();
  replaced.reassignedWrapperOnly();
  console.log(typeof setup);
};
