import React from "react";
import { createRoot } from "react-dom/client";
import { Button, PrimaryButton } from "./styled";
import { Box, liveStyle } from "./emotion";
import { container } from "./theme.css";

export const App: React.FC = () => (
  <div className={container}>
    <Button>Click</Button>
    <PrimaryButton>Primary</PrimaryButton>
    <Box css={liveStyle}>Boxed</Box>
  </div>
);

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(<App />);
}
