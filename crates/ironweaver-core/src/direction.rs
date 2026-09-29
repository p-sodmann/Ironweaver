// direction.rs

use std::str::FromStr;

use crate::GraphError;

/// Which edges a traversal follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    /// Outgoing edges (source -> target).
    #[default]
    Out,
    /// Incoming edges, walked backwards.
    In,
    /// Both, ignoring edge direction.
    Both,
}

impl Direction {
    /// Parse `"out"`, `"in"` or `"both"`; `None` means `"out"`.
    pub fn parse(direction: Option<&str>) -> Result<Self, GraphError> {
        direction.unwrap_or("out").parse()
    }

    /// The direction that walks the same edges backwards.
    pub fn reversed(self) -> Self {
        match self {
            Direction::Out => Direction::In,
            Direction::In => Direction::Out,
            Direction::Both => Direction::Both,
        }
    }
}

impl FromStr for Direction {
    type Err = GraphError;

    fn from_str(s: &str) -> Result<Self, GraphError> {
        match s {
            "out" => Ok(Direction::Out),
            "in" => Ok(Direction::In),
            "both" => Ok(Direction::Both),
            other => {
                Err(GraphError::InvalidArgument(format!("direction must be 'out', 'in' or 'both', got '{}'", other)))
            }
        }
    }
}
