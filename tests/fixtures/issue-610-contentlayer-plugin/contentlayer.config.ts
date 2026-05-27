import { defineDocumentType, makeSource } from "contentlayer2/source-files";
import rehypeKatex from "rehype-katex";
import rehypeSlug from "rehype-slug";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";

export const Blog = defineDocumentType(() => ({
  name: "Blog",
  filePathPattern: "blog/**/*.mdx",
  contentType: "mdx",
}));

export const Authors = defineDocumentType(() => ({
  name: "Authors",
  filePathPattern: "authors/**/*.mdx",
  contentType: "mdx",
}));

export default makeSource({
  contentDirPath: "data",
  documentTypes: [Blog, Authors],
  mdx: {
    remarkPlugins: [remarkGfm, remarkMath],
    rehypePlugins: [rehypeSlug, [rehypeKatex, {}]],
  },
});
