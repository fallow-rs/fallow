export default {
  git: { commitMessage: "chore: release v${version}" },
  plugins: {
    "@release-it/conventional-changelog": { preset: "conventionalcommits" },
    "release-it-sample-plugin": {},
    "./scripts/release-plugin.js": {},
  },
};
