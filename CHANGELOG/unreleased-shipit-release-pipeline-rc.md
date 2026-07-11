### Changed

- Verified the shipit release pipeline end-to-end with a throwaway
  `-release-rc` cut through the composed `wf-release.yml` (shipit
  TOL02-WS07): preflight → prepare → build → bundle → assert-bundle →
  publish, gh-release prerelease only (crates/npm/brew skipped by the
  central RC guard).
