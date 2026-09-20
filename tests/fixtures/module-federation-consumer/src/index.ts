import "checkout";
import "checkout-ui";
import "genuinely-missing-pkg";

export const mount = async (): Promise<unknown> => {
  const remote = await import("checkout/Button");
  return remote;
};
