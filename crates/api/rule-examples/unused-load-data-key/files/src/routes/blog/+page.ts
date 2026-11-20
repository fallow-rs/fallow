import type { PageLoad } from "./$types";

export const load: PageLoad = async ({ fetch }) => {
  const posts: { title: string }[] = await (await fetch("/api/posts")).json();
  return {
    posts,
    draftCount: posts.length,
  };
};
