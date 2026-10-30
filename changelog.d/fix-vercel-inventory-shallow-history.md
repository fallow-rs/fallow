### Fixed

- **Inventory uploads omit churn from shallow Git history.** Partial histories are omitted on both fresh and cached uploads.
- **Cloud analysis treats managed partial captures as positive execution evidence.** Human and Markdown reports withhold application coverage percentages and cold-code conclusions. JSON reports set `coverage_percent`, `functions_tracked` and `functions_unhit` to null.

### Added

- **Inventory and source-map uploads accept a Vercel deployment binding.** Use `--vercel-deployment-id`, or `VERCEL_DEPLOYMENT_ID` during a Vercel build. When the upload authenticates with Vercel OIDC, the cloud verifies the deployment against the upload SHA.
