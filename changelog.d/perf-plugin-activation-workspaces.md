### Performance

- **Plugin detection is faster in monorepos with many workspaces.** For each workspace, some plugins look for their config file anywhere below the workspace root. Before, each of these checks compared every folder of the project with the workspace root. Now each check reads only the folders of that workspace. On babel (174 workspaces), the plugin stage of a cold `dead-code` run takes about 38% less time. The results do not change.
