export const dynamic = "force-static";
export const revalidate = 3600;
export const fetchCache = "force-cache";
export const runtime = "nodejs";
export const preferredRegion = "auto";
export const maxDuration = 30;

export const dynamicParams = true;
export const unusedSitemapHelper = "unused";

export default function sitemap() {
  return [{ url: "https://example.com" }];
}
