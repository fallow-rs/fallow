declare function memo<T>(component: T): T;
declare function createContext<T>(value: T): { value: T };
declare function forwardRef<T, P>(render: (props: P, ref: T) => unknown): (props: P) => unknown;

export interface MemoProps {
  title: string;
}

export const Memo = memo(function Memo({ title }: MemoProps) {
  return title;
});

export interface RefProps {
  label: string;
}

export const Ref = forwardRef<HTMLDivElement, RefProps>((props) => props.label);

export interface ContextValue {
  count: number;
}

export const Context = createContext<ContextValue | null>(null);

export type Mode = "on" | "off";

export const DEFAULT_MODE = "on" as Mode;

type LocalState = {
  ready: boolean;
};

export const StateContext = createContext<LocalState | null>(null);

export interface HiddenValue {
  hidden: number;
}

const hiddenContext = createContext<HiddenValue | null>(null);

export const hasHidden = (): boolean => hiddenContext.value !== null;
