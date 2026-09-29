const workerPath = require.resolve('./worker.js');
const manifestPath = require.resolve('../package.json');
const searchedPath = require.resolve('./searched.js', { paths: [process.cwd()] });
const loaderPath = require.resolve('./loader.js');
const templatePath = require.resolve(`./template-target.js`);
const builtPath = require.resolve('../dist/built-worker.js');

module.exports = {
  workerPath,
  manifestPath,
  searchedPath,
  loaderPath,
  templatePath,
  builtPath,
};
