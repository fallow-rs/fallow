### Fixed

- **Ownership counts one person once when `.mailmap` joins two emails.**
  Churn history now reads the mailmap-resolved author email. The churn
  cache is rebuilt once, and again after an edit of `.mailmap`.
- **`health.ownership.botPatterns` matches the local part of the author
  email.** A pattern such as `release-bot` now excludes
  `release-bot@corp.com`. A pattern that matches the full address keeps
  its result.
- **Two contributors no longer get one identifier in handle mode.** A plus
  tag stays in the handle (`john+github`), and only a GitHub noreply
  address `12345+name@users.noreply.github.com` gives `name`. Identifiers
  in health JSON change for plus-addressed emails.
- **Reviewer routing no longer names the reviewer.** The audit brief
  compares the git identity of the reviewer with the contributor itself,
  so the `anonymized` mode and disambiguated handles do not hide a match.
