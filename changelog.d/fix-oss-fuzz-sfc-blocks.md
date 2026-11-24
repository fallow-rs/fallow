### Fixed

- **A Vue or Svelte file with many unclosed attribute quotes can no longer slow the analysis to a crawl.** Fallow found the `<script>` and `<style>` blocks of a single-file component with regexes. A tag start whose quoted attribute value did not close made each search read far past its block, and the next search read that text again, so the time grew with the square of the file size. A 125 KB file took minutes in a fuzz build. Fallow now finds the blocks with a scan that reads each position a bounded number of times. The blocks that it finds do not change.
