# rbun-macros

Procedural macros for [rbun](https://github.com/KeyChord/rbun), the Rust bindings
for Bun's JavaScript runtime.

Use the `rbun` crate with its default `macro` feature to access
`#[rbun::class]`, `#[rbun::methods]`, `rbun::derive::Trace`, and
`rbun::derive::JsLifetime`. This crate is normally an indirect dependency.

```toml
[dependencies]
rbun = "0.1.0"
```
