import styled from "@emotion/styled";
import { css } from "@emotion/react";

// LIVE: rendered by main.tsx
export const Box = styled.div`
  display: flex;
  gap: 8px;
`;

// DEAD: exported, used nowhere
export const deadStyle = css`
  opacity: 0.5;
`;

// LIVE: used by main.tsx
export const liveStyle = css`
  opacity: 1;
`;
