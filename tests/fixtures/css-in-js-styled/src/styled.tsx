import styled from "styled-components";

// LIVE: imported and rendered by main.tsx
export const Button = styled.button`
  color: white;
  background: rebeccapurple;
  padding: 8px 16px;
`;

// DEAD: exported, imported nowhere
export const DeadCard = styled.div`
  border: 1px solid #ccc;
  border-radius: 4px;
  padding: 16px;
`;

// LIVE: styled(Component) wrapping form, rendered by main.tsx
export const PrimaryButton = styled(Button)`
  font-weight: bold;
`;
