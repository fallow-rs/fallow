### Performance

- **The plugin stage is faster on large monorepos.** Fallow looks for some plugin config files in folders that the file walk skips, such as hidden folders. It does this in each folder that holds a source file. Before, each of these lookups did a full path check, also when the folder did not exist. Now one `stat` call rules out a missing folder first. On next.js, the plugin stage of a cold `dead-code` run takes about 60% less time, and the full run about 20% less. The results do not change.
