module.exports.readVersion = (contents) => {
  const match = contents.match(/versionName "(.*)"/);
  return match ? match[1] : "0.0.0";
};
module.exports.writeVersion = (contents, version) =>
  contents.replace(/versionName ".*"/, `versionName "${version}"`);
