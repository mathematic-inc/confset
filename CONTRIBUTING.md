# Contributing

AI agents handle most maintenance and implementation for this repository. For us,
reviewing an unsolicited pull request takes longer than implementing a proposal
after we have agreed on it.

## Propose a change

1. [Start a GitHub Discussion](https://github.com/mathematic-inc/confset/discussions/new)
   describing the problem or idea.
2. Wait for a Mathematic maintainer to review the proposal before doing
   implementation work.
3. If we accept the proposal, a Mathematic maintainer or agent will open the pull
   request.

When Mathematic implements a proposal, we will link the implementation pull request
to the Discussion and credit the original author.

GitHub restricts pull request creation to Mathematic maintainers and repository
collaborators who have write, maintain, or admin access, plus authorized
maintenance agents.

## Development

Install the pinned tools with `mise install`. Run the checks used in CI:

```sh
mise exec -- hk check --all --slow
mise exec -- cargo test --locked
mise exec -- cargo build --release --locked
```

CI uses Rust 1.98.1, including its minimum-version check:

```sh
rustup run 1.98.1 cargo check --locked --workspace --all-features --all-targets
```

Keep development caches, build outputs, and disposable test projects on the
development volume selected by your workspace policy.
