//! Contract V2.0.2 rules core: a line-by-line port of the pinned TypeScript core (`artifacts/v202/scripts/core/src`).
//! Pure computation, no I/O. Differential-tested against transitions recorded by the JS build.
pub mod accepted;
pub mod artifacts;
pub mod bytes;
pub mod covenant;
pub mod genesis;
pub mod hashes;
pub mod protocol;
pub mod rolling;
pub mod state;
pub mod transaction;
