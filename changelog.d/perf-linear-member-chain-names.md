### Performance

- **Very long member chains extract much faster.** Extraction built the dotted name of a member chain again at each link, and each build copied the name once per link. A file with a chain of 4000 links (`a.b.b.b...`) took several seconds to extract. Fallow now builds each name in one pass, so the same file takes a fraction of a second. Real code does not have chains this long, but generated or hostile input can. The extraction output does not change.
