import AnonymousRenderer from "./anonymous-renderer";
import AssignedRenderer from "./assigned-renderer";
import DeclaredRenderer from "./declared-renderer";
import { useEffect, useRef } from "./react";

export function AssignedPlot({ n }: { n: number }) {
  const renderer = useRef<AssignedRenderer>(null);
  useEffect(() => {
    const created = new AssignedRenderer();
    renderer.current = created;
    return () => created.destroy();
  }, []);
  useEffect(() => {
    renderer.current?.update(n);
  }, [n]);
}

export function DeclaredPlot({ n }: { n: number }) {
  const renderer = useRef<DeclaredRenderer>(null);
  useEffect(() => {
    const created = new DeclaredRenderer();
    renderer.current = created;
    return () => created.destroy();
  }, []);
  useEffect(() => {
    renderer.current?.update(n);
  }, [n]);
}

export function AnonymousPlot({ n }: { n: number }) {
  const renderer = useRef<AnonymousRenderer>(null);
  useEffect(() => {
    const created = new AnonymousRenderer();
    renderer.current = created;
    return () => created.destroy();
  }, []);
  useEffect(() => {
    renderer.current?.update(n);
  }, [n]);
}
