export const sendCheckoutToken = (token: string) => {
  window.parent.postMessage({ token }, "*");
};
