const workerPath = require.resolve('./worker.js');
const manifestPath = require.resolve('../package.json');
const searchedPath = require.resolve('./searched.js', { paths: [process.cwd()] });

module.exports = { workerPath, manifestPath, searchedPath };
