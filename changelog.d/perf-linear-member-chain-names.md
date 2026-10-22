### Performance

- **Long member chains no longer slow down extraction.** Extraction built the dotted name of a member chain again at each link, and each build copied the name once per link. A file with a chain of 4000 links (`a.b.b.b...`) took seconds to parse. Fallow now builds each name in one pass, so the same file takes a fraction of a second. The extraction output does not change.
