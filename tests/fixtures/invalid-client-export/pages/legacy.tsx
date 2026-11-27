"use client";

// The Pages Router gives the directive no meaning, so the data function is valid.
export const getServerSideProps = async () => ({ props: {} });

export default function Legacy() {
  return <div>Legacy</div>;
}
