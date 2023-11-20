# actor-rs

Simple Rust Actors

## Notes

- `crossbeam-channel` has been used purely for its `select!` macro
  - If this lands in rust `std` consider removing dependency on this library
