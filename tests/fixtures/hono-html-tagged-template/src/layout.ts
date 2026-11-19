// A Hono-style layout that emits HTML via the `html` tagged template literal
// (not JSX). The asset references inside the template must be tracked so
// sibling files in `static/` stay reachable from this entry point.
//
// Models the exact pattern from issue #105 (till's follow-up comment): a
// `.ts` file, no JSX, bare `html` tag. The tag comes from `hono/html`; fallow
// reads a tag that the module declares itself as plain text (issue #4730).
import { html } from 'hono/html';

export const Layout = ({ title, body }: { title: string; body: string }) => html`
  <!doctype html>
  <html>
    <head>
      <title>${title}</title>
      <meta name="viewport" content="width=device-width, initial-scale=1" />
      <link rel="stylesheet" href="/static/style.css" />
      <link rel="modulepreload" href="/static/vendor.js" />
      <script defer src="/static/otp-input.js"></script>
    </head>
    <body>${body}</body>
  </html>
`;
