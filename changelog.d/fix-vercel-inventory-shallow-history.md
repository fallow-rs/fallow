### Fixed

- **Inventory uploads omit churn from shallow Git history.** Partial histories are omitted on both fresh and cached uploads.
- **Cloud analysis treats managed partial captures as positive execution evidence.** Human and Markdown reports withhold application coverage percentages and cold-code conclusions.

### Added

- **Inventory and source-map uploads accept a Vercel deployment binding.** Use `--vercel-deployment-id`, or `VERCEL_DEPLOYMENT_ID` during a Vercel build. The cloud verifies the deployment against the upload SHA.
