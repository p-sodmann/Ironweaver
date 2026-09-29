// vertex/mod.rs

mod algorithms;
mod analysis;
mod batch;
pub(crate) mod callbacks;
mod core;
mod manipulation;
mod pathfinding;
mod serialization;
pub(crate) mod subgraph;

pub use core::Vertex;
