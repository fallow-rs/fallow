const { danger, warn } = require("danger");

if (danger.github.pr.title.length < 5) {
  warn("PR title is too short");
}
