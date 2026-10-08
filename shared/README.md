# Cooked data formats

`hk-format` validates the bounded little-endian cooked room representation.
`hk-sim` implements allocation-free deterministic Q16.16 gameplay using that
format. Both have native host tests and are consumed by the no_std guest.
See `../docs/FORMAT.md`; host pointers and Rust memory layouts are never stored.
