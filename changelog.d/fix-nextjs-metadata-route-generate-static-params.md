### Fixed

- **`fallow dead-code` no longer reports `generateStaticParams` in a Next.js metadata route as an unused export.** Next.js calls it to prerender an `opengraph-image`, `twitter-image`, `icon`, `apple-icon`, `sitemap`, `manifest` or `robots` route under a dynamic segment, such as `app/blog/[slug]/opengraph-image.tsx`. It is now credited with the route segment config exports (`dynamic`, `revalidate`, `fetchCache`, `runtime`, `preferredRegion`, `maxDuration`). `dynamicParams` and unrelated helper exports stay reportable.
