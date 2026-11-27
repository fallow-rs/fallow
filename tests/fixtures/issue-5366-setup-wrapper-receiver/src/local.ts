export class LocalContext {
  localWrapperUsed(): void {}

  localUnused(): void {}
}

const newLocal = () => {
  const ctx = new LocalContext();
  return { ctx };
};

const setupLocal = () => newLocal();

export const runLocal = (): void => {
  const { ctx: local } = setupLocal();
  local.localWrapperUsed();
};
