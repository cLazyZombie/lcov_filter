# Security

`lcov_filter` is a local command-line tool. It reads LCOV data from stdin and
reads the source files referenced by `SF:` records to find exclusion markers.

Use it with trusted coverage output and trusted local source trees. It does not
perform network access or execute commands, but a crafted LCOV file can cause it
to read arbitrary local paths named in `SF:` records.

To report a security issue, open a private advisory on GitHub or contact the
maintainer listed on the repository.
