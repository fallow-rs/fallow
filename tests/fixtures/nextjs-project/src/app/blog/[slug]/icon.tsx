export const size = { width: 32, height: 32 };
export const contentType = "image/png";

export function generateStaticParams() {
  return [{ slug: "a" }];
}

export const unusedImageHelper = "unused";

export default function Icon() {
  return new Response("icon");
}
