// vertex/mod.rs

mod core;
mod callbacks;
mod manipulation;
mod serialization;
mod analysis;
pub(crate) mod algorithms;
pub(crate) mod pathfinding;
pub(crate) mod subgraph;

pub use core::Vertex;
