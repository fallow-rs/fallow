import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { CheckoutForm } from "./CheckoutForm";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <CheckoutForm />
  </StrictMode>,
);
