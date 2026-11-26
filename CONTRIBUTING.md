# Contributing to Fundamental

Thanks for working on Fundamental. [`AGENTS.md`](AGENTS.md) holds the rules every change follows,
whether a person or a coding agent writes it. This page says how to propose one.

## Ways to contribute

- **Report a bug or ask a question**: open a [GitHub issue](https://github.com/sounds-global/fundamental/issues).
- **Propose a change**: open an issue for anything larger than a fix, so the design can be agreed
  before the code is written. See [`GOVERNANCE.md`](GOVERNANCE.md).
- **Write code**: pick an open issue, or ask on one before starting something large.

Security problems are never reported in public issues. See [`SECURITY.md`](SECURITY.md).

## Before you start

```sh
git config core.hooksPath .githooks   # signs off each commit and holds it to the rule in AGENTS.md
```

## Developer Certificate of Origin

Every commit must be signed off under the [Developer Certificate of Origin](DCO) 1.1. By signing
off, you certify that you wrote the change, or otherwise have the right to submit it under the
project's license. Add the sign-off with `-s`:

```sh
git commit -s -m "fix(sync): keep the slice cursor across a reconnect"
```

This adds a line with your real name and email:

```text
Signed-off-by: Jane Doe <jane@example.com>
```

The repository's hooks add it for you (`git config core.hooksPath .githooks`, under
"Before you start").

A check on every pull request rejects commits without a sign-off. To fix earlier commits on your
branch, run `git rebase --signoff main` and push again.

## Licenses of contributions

By submitting a contribution you agree it is licensed under the
[Apache License, Version 2.0](LICENSE), the license of this project.
Do not copy code from GPL, LGPL or AGPL projects, and list any new third-party dependency, with
its license, in the pull request.

## Documentation is part of the change

A change is not done until its docs and its [`CHANGELOG.md`](CHANGELOG.md) entry are in the
same commit.

| You are writing… | It goes in… |
|------------------|-------------|
| What a type, trait, function or macro is and how to call it | A `///` doc comment next to the item |
| A crate's purpose and entry points | The crate's `//!` header in `lib.rs` |

- **Every public item is documented.** `missing_docs` is enforced; a new public item without a
  `///` comment fails the build on a crate that denies it.
- **Examples are compiled.** Write examples as ` ```rust ` doctests. ` ```no_run ` is for an
  example that needs a live resource (a database, Vault); ` ```ignore ` only for a fragment that
  cannot compile alone.
- **Link, don't repeat.** Intra-doc links (`` [`Model`] ``) keep references valid; a broken one
  fails the build.
- **Feature-gated items show their requirement** with
  `#[cfg_attr(docsrs, doc(cfg(feature = "...")))]`.
- **The umbrella crate's front page is `README.md`**, included by `src/lib.rs`: edit the README.

## Checks before a pull request

```sh
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo test --doc --workspace --all-features
cargo build --examples --workspace --all-features

# Every crate with an optional `std` feature still builds without it.
for crate in fundamental_core fundamental_i18n_keys fundamental_config \
             fundamental_db fundamental_i18n; do
  cargo clippy -p "$crate" --no-default-features -- -D warnings || break
done
```

The default nextest profile leaves out the tests that start a database container or a browser;
`cargo nextest run --workspace --profile release` runs them too, and needs Docker.

A change to a hot path also runs `fun bench`: its instruction counts are checked against the
committed baseline on every pull request, and a count that moves on purpose is saved with
`fun bench --save` and committed with the reason. [`BENCHMARKS.md`](BENCHMARKS.md) describes
every layer (`fun bench wall`, `load`, `soak`, `fault`) and how to reproduce its numbers.

A doctest that fails after you rename a field or change a signature is the system working: fix
the example.

By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).
