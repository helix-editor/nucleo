pub mod main {
    pub mod boxcar;
}
pub use main::*;

criterion::criterion_main!(boxcar::benches);
