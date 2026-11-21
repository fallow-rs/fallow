import * as S from "effect/Schema";

const Text = S.String;

const makeNamed = () => S.Struct({ name: Text });

function makeFn() {
  return S.Struct({ name: Text });
}

const makeArg = (key: string) => S.Struct({ [key]: Text });

function makeEither(strict: boolean) {
  if (strict) {
    return S.Struct({ name: Text });
  }
  return S.Struct({});
}

function makeWithLocal() {
  const base = S.Struct({ name: Text });
  return base.pipe(S.annotations({ title: "Local" }));
}

function makeNested() {
  return makeNamed().annotations({ title: "Nested" });
}

export const FromHelper = makeNamed();
export const FromFn = makeFn();
export const FromArg = makeArg("x");
export const FromEither = makeEither(true);
export const FromLocal = makeWithLocal();
export const FromNested = makeNested();
export const FromHelperPipe = makeNamed().pipe(S.annotations({ title: "Piped" }));

const plainHelper = () => 1;

function plainFn() {
  return { name: "plain" };
}

const plainArg = (key: string) => ({ [key]: "plain" });

function mixedReturns(strict: boolean) {
  if (strict) {
    return S.Struct({ name: Text });
  }
  return "plain";
}

function openEnd(strict: boolean) {
  if (strict) {
    return S.Struct({ name: Text });
  }
}

function plainLocal() {
  const base = { pipe: (): number => 1 };
  return base.pipe();
}

function plainNested() {
  return plainHelper();
}

export const NotSchema = (() => 1)();
export const fromPlainHelper = plainHelper();
export const fromPlainFn = plainFn();
export const fromPlainArg = plainArg("x");
export const fromMixedReturns = mixedReturns(true);
export const fromOpenEnd = openEnd(true);
export const fromPlainLocal = plainLocal();
export const fromPlainNested = plainNested();
export const fromDecoderPipe = makeNamed().pipe(S.decodeUnknownSync);
