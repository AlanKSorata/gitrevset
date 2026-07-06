# gitrevset

[![Documentation](https://docs.rs/gitrevset/badge.svg)](https://docs.rs/gitrevset)
![Build Status](https://github.com/quark-zju/gitrevset/workflows/build/badge.svg)

A domain-specific-language to select commits in a git repo. Similar to
[Mercurial's revset](https://www.mercurial-scm.org/repo/hg/help/revsets).

See [the crate documentation](https://docs.rs/gitrevset/#language-specification) for supported functions and operators. More functions might be added over time.

`gitrevset` provides the Rust library interface. There is also a simple command-line utility `git-revs`. It takes revset expressions as arguments, and outputs commit hashes.

## Examples

### Revset Expressions

The current commit (HEAD) and its parent:

    . + .^

Merge base (common ancestor) of HEAD and origin/master:

    gca(., origin/master)

The bottom of the current local (draft) branch:

    roots(draft() & ::.)

Tagged commits since 100 days ago:

    tag() & date("since 100 days ago")

Commits by "alice" or "bob" in the "dev" but not "master" branch:

    (dev % master) & (author(alice) | author(bob))

### Using `gitrevset` Library

**Open or clone a repository**, then run revset queries:

```rust
use gitrevset::{Repo, SetExt};

// Open from the current repository (GIT_DIR or cwd).
let repo = Repo::open_from_env()?;

// Open from an explicit path.
let repo = Repo::open("/path/to/repo")?;

// Clone a remote repository and build the commit graph index.
let repo = Repo::clone("https://github.com/rust-lang/rust.git", "/tmp/rust")?;

// Fetch updates from a remote and refresh the index.
repo.fetch("origin")?;

// Run revset queries.
let set = repo.revs("(draft() & ::.)^ + .")?;
for oid in set.to_oids()? {
    dbg!(oid?)
}
```

Parse at compile time. Interact with local variables like strings, or calculated set:

```rust
use gitrevset::{ast, Repo};

let repo = Repo::open_from_env()?;
let master = "origin/master";
let stack = repo.revs(ast!(only(".", ref({ master }))))?;
let head = repo.revs(ast!(heads({ stack })))?;
```

### Using `git-revs` CLI

Query the current repository:

```bash
git revs "(draft() & ::.)^ + ."
```

Query a repository at a specific path:

```bash
git revs --open /path/to/repo "head()"
```

Clone a remote repository and query it:

```bash
git revs --clone https://github.com/rust-lang/rust.git /tmp/rust "all()"
```

Fetch updates from a remote and re-run queries:

```bash
git revs --fetch origin "draft()"
```

### Configuration

Customized revset aliases or functions can be defined in git config:

```ini
[revsetalias]
d = draft()
f = ancestor($1, origin/master):$1
```

Then they can be used in `git-revs` or using the `repo.anyrevs` API.

```bash
git revs "f(d)"
```
