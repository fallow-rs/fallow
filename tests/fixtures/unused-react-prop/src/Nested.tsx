// A NON-exported component with a nested destructure (`{ user: { name } }`):
// fallow cannot flatten the local with confidence, so the whole component
// abstains. `name` is read; `dead` is not, but neither is flagged.
const Inner = ({ user: { name }, dead }: { user: { name: string }; dead: string }) => (
  <i>{name}</i>
);

export const Nested = ({ user }: { user: { name: string } }) => (
  <Inner user={user} dead="d" />
);
