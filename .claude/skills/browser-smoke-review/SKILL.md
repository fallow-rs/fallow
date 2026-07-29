---
name: browser-smoke-review
description: Use browser automation to review docs pages, preview URLs, rendered output, or web-facing fallow surfaces. Use when the user wants a screenshot-based review, browser smoke test, docs site check, or preview deployment inspection.
---
<!-- Generated from .agents/skills. Do not edit. -->

# Browser Smoke Review

Use Playwright/browser tooling for web-facing checks.

Good fits:
- docs site checks
- preview deployment inspection
- rendered markdown or report review
- screenshot capture for review
- browser console and network smoke tests

Workflow:
1. Open the target URL or local preview.
2. Inspect the page structure and visible content.
3. Capture screenshots when useful.
4. Check console errors and obvious network failures.
5. Report concrete UX or correctness issues with page/path context.

If the user wants product feedback rather than just smoke testing, combine with `panel-review`.
