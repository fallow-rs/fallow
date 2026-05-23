import type { Route } from "./+types/root";
import { runtimeValue } from "./+types/runtime";

export async function loader() {
  return null;
}

export async function clientLoader() {
  return null;
}

export async function clientAction() {
  return null;
}

export function Layout({ children }: Route.ComponentProps) {
  return children;
}

export function HydrateFallback() {
  return null;
}

export function ErrorBoundary() {
  return null;
}

export function shouldRevalidate() {
  return true;
}

export const handle = { scope: "root" };
export const unusedRootHelper = () => null;
void runtimeValue;

export default function Root() {
  return null;
}
