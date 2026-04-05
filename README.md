# arcweave-rust

Rust runtime for running [Arcweave](https://arcweave.com) interactive stories.

Parses Arcweave's JSON export format and evaluates Arcscript — including variables, conditions, branching, and visit tracking — so you can drive Arcweave node trees from a Rust game engine.

## Usage

```rust
use arcweave_rust::{Runtime, project::Project};

// Load a project exported from Arcweave
let project = Project::from_file("my_project.json")?;
let mut runtime = Runtime::new(&project);


// Render the current element's content
if let Some(content) = runtime.render_current_content()? {
    // walk the Content tree and display it
}

// Render available options and let the player choose
let options = runtime.render_current_options()?;

// Follow a chosen connection
runtime.follow(&chosen_conn_ref)?;

// Save and restore state
let saved = runtime.save()?;
runtime.load(&saved)?;
```

## Project structure

```
src/
├── lib.rs          — Runtime, RuntimeState, RuntimeVariable, Content
├── project/        — Project deserialization (elements, connections, branches, variables, ...)
└── script/
    ├── mod.rs      — Environment, expression evaluator
    ├── ast.rs      — Arcscript AST types
    └── parser.rs   — nom-based ArcScript parser
tests/              — Arcweave project JSON files used in integration tests
```

## Getting started with Arcweave

Export your project from [arcweave.com](https://arcweave.com) as JSON (`File → Export → JSON`), then load it with `Project::from_file`.

## Dependencies

- [`nom`](https://github.com/rust-bakery/nom) — ArcScript parser
- [`serde` / `serde_json`](https://serde.rs) — project deserialization and state serialization
- [`thiserror`](https://github.com/dtolnay/thiserror) — error types
- [`html-escape`](https://crates.io/crates/html-escape) — decodes HTML entities in ArcScript strings from the JSON export
- [`rand`](https://crates.io/crates/rand) — `random()` and `roll()` ArcScript functions

## License

MIT or Apache-2.0