### Performance

- **A warm run no longer reads the metadata of each unchanged file a second time.** When the parse cache serves a file from its metadata alone, the cache update after the parse keeps the stored entry and makes no new metadata read for that file. Before, the update read the metadata of every file again on one thread, also on a run where no file changed.
