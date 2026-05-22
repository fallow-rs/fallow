import "./jsdoc-consumer.js";

// A Hono-style layout component that emits HTML via JSX. These resource
// attributes are runtime HTML metadata and should not become module imports.
export const Layout = () => (
  <html>
    <head>
      <link rel="stylesheet" href="/static/style.css" />
      <link rel="modulepreload" href="/static/vendor.js" />
      <script src="/static/app.js"></script>
    </head>
    <body>
      <h1>Hello</h1>
    </body>
  </html>
);
