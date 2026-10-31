export const alt = "Blog post";
export const size = { width: 1200, height: 630 };
export const contentType = "image/png";

export function generateStaticParams() {
  return [{ slug: "a" }];
}

export const unusedImageHelper = "unused";

export default function Image() {
  return new Response("image");
}
