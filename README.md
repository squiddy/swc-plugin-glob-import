# swc-plugin-glob-import

We've been making use of babel plugins to have glob imports, i.e. `import images
from "cats*.jpg";` in a project of work. We considered moving over to NextJS,
which, at that time (end of 2022), was using SWC and didn't have support for
this yet.

This never left the experimental stage, but served as a learning experience for
me when it came to SWC and Rust.

If you're looking for something usable, head over to [jcoon97/swc-import-glob-array-plugin](https://github.com/jcoon97/swc-import-glob-array-plugin).
