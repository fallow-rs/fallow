const workerPath = require.resolve('./worker.js');
const manifestPath = require.resolve('../package.json');
const searchedPath = require.resolve('./searched.js', { paths: [process.cwd()] });
const loaderPath = require.resolve('./loader.js');

module.exports = { workerPath, manifestPath, searchedPath, loaderPath };
