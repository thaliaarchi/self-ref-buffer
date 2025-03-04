# Persistent buffers and self-referential types

An experiment in combining persistent buffers and self-referential types in
Rust.

This has a minimal example of a buffered reader type which uses persistent,
reference-counted buffers. Once data is written to a buffer, it cannot be
overwritten. Shared borrows can be retained to the filled portion while writing
into the untilled portion. When a buffer is filled or has insufficient space,
the reader constructs a new buffer.

I want pair it with [`yoke`](https://docs.rs/yoke/latest/yoke/)-like
self-referential structs. The owner is the borrowed buffers (specifically, a vec
of reference counted references to them). The borrowed fields are
`&'owner [u8]`. Currently, this part does not compile.

This is an experiment, so its API is deliberately minimal and the buffer and
reader types are hacked together for demonstration.
