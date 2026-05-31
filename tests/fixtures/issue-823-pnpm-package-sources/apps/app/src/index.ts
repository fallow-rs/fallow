import { registryValue } from "registry-pkg";
import { jsrValue } from "jsr-pkg";
import { workspaceValue } from "workspace-pkg";
import { localDirValue } from "local-dir-pkg";
import { localTarballValue } from "local-tarball-pkg";
import { remoteTarballValue } from "remote-tarball-pkg";
import { gitUrlValue } from "git-url-pkg";
import { gitShorthandValue } from "git-shorthand-pkg";
import { npmAliasValue } from "npm-alias-pkg";

console.log(
  registryValue,
  jsrValue,
  workspaceValue,
  localDirValue,
  localTarballValue,
  remoteTarballValue,
  gitUrlValue,
  gitShorthandValue,
  npmAliasValue,
);
