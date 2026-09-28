// pathfinding/cost.rs

use crate::{Attributes, GraphError, Lookup};

/// How much traversing an edge costs.
#[derive(Clone, Debug, PartialEq)]
pub enum EdgeCost {
    /// Every edge costs 1 (hop count).
    Unit,
    /// The edge attribute `key`, or `default` when the edge doesn't have it.
    Weighted { key: String, default: f64 },
}

impl EdgeCost {
    /// Weighted by attribute `key` (default `"weight"`), `default` (default
    /// 1.0) for edges without it.
    pub fn weighted(key: Option<String>, default: Option<f64>) -> Self {
        EdgeCost::Weighted {
            key: key.unwrap_or_else(|| "weight".to_string()),
            default: default.unwrap_or(1.0),
        }
    }

    /// Cost of an edge with payload `edge`; weights must be non-negative
    /// numbers.
    pub fn cost<E, X>(&self, edge: &E) -> Result<f64, X>
    where
        E: Attributes,
        X: From<GraphError> + From<E::Error>,
    {
        let (key, default) = match self {
            EdgeCost::Unit => return Ok(1.0),
            EdgeCost::Weighted { key, default } => (key, *default),
        };
        let w = match edge.number(std::slice::from_ref(key))? {
            Lookup::Found(w) => w,
            Lookup::Missing => default,
            Lookup::Invalid => {
                return Err(GraphError::InvalidType(format!("Edge attribute '{}' must be a number", key)).into())
            }
        };
        if w.is_nan() || w < 0.0 {
            return Err(GraphError::InvalidArgument(format!("Edge weights must be non-negative, got {}", w)).into());
        }
        Ok(w)
    }
}
