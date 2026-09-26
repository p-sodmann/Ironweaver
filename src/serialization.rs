// serialization.rs
use pyo3::prelude::*;
use pyo3::types::{
    PyAny, PyBool, PyDict, PyFloat, PyInt, PyList, PyMapping, PySequence, PyString, PyTuple,
};
use serde::{Deserialize, Serialize};
use half::f16;
use serde::ser::{SerializeStruct, Serializer as _};
use bincode::Options;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use crate::{Node, Edge, Vertex};

/// Serializable representation of a node that avoids circular references
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SerializableNode {
    pub id: String,
    pub attr: HashMap<String, SerializableValue>,
    pub meta: HashMap<String, SerializableValue>,
    pub edge_ids: Vec<String>, // Store edge IDs instead of actual edges
    pub inverse_edge_ids: Vec<String>, // Store inverse edge IDs
}

/// Serializable representation of an edge
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SerializableEdge {
    pub id: String, // Unique edge identifier
    pub from_id: String,
    pub to_id: String,
    pub attr: HashMap<String, SerializableValue>,
    pub meta: HashMap<String, SerializableValue>,
}

/// Serializable representation of Python values
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum SerializableValue {
    String(String),
    Int(i64),
    Float(f64),
    Half(f16),
    Bool(bool),
    None,
    List(Vec<SerializableValue>),
    Dict(HashMap<String, SerializableValue>),
}

/// Complete graph representation for serialization
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SerializableGraph {
    pub nodes: HashMap<String, SerializableNode>,
    pub edges: HashMap<String, SerializableEdge>,
    pub meta: HashMap<String, SerializableValue>,
    pub metadata: HashMap<String, SerializableValue>,
}

/// Dictionary keys are serialized as strings; non-string keys use `str(key)`.
fn dict_key(key: &Bound<'_, PyAny>) -> PyResult<String> {
    match key.downcast::<PyString>() {
        Ok(s) => Ok(s.to_str()?.to_owned()),
        Err(_) => key.str()?.extract(),
    }
}

fn attr_map(py: Python<'_>, map: &HashMap<String, Py<PyAny>>) -> PyResult<HashMap<String, SerializableValue>> {
    let mut out = HashMap::with_capacity(map.len());
    for (key, value) in map {
        out.insert(key.clone(), SerializableValue::from_bound(value.bind(py))?);
    }
    Ok(out)
}

/// Convert a Python object to a `serde_json::Value` directly (used to load a
/// graph from a dict without a `json.dumps` + parse round trip).
pub fn py_to_json(obj: &Bound<'_, PyAny>) -> PyResult<serde_json::Value> {
    use serde_json::Value;
    if obj.is_none() {
        Ok(Value::Null)
    } else if let Ok(b) = obj.downcast::<PyBool>() {
        Ok(Value::Bool(b.is_true()))
    } else if let Ok(i) = obj.downcast::<PyInt>() {
        match i.extract::<i64>() {
            Ok(v) => Ok(Value::from(v)),
            Err(_) => Ok(Value::from(i.extract::<f64>()?)),
        }
    } else if let Ok(f) = obj.downcast::<PyFloat>() {
        Ok(serde_json::Number::from_f64(f.value()).map(Value::Number).unwrap_or(Value::Null))
    } else if let Ok(s) = obj.downcast::<PyString>() {
        Ok(Value::String(s.to_str()?.to_owned()))
    } else if let Ok(dict) = obj.downcast::<PyDict>() {
        let mut map = serde_json::Map::with_capacity(dict.len());
        for (k, v) in dict.iter() {
            map.insert(dict_key(&k)?, py_to_json(&v)?);
        }
        Ok(Value::Object(map))
    } else if let Ok(list) = obj.downcast::<PyList>() {
        list.iter().map(|v| py_to_json(&v)).collect::<PyResult<Vec<_>>>().map(Value::Array)
    } else if let Ok(tuple) = obj.downcast::<PyTuple>() {
        tuple.iter().map(|v| py_to_json(&v)).collect::<PyResult<Vec<_>>>().map(Value::Array)
    } else {
        Err(pyo3::exceptions::PyTypeError::new_err(format!(
            "Unsupported value in graph dict: {}",
            obj.get_type().name()?
        )))
    }
}

impl SerializableValue {
    /// Convert Python object to SerializableValue
    pub fn from_python(py: Python<'_>, obj: &Py<PyAny>) -> PyResult<Self> {
        Self::from_bound(obj.bind(py))
    }

    /// Convert a bound Python object to SerializableValue.
    ///
    /// Dispatches on the concrete Python type (checking `bool` before `int`,
    /// since `bool` is an `int` subclass) instead of trying a chain of
    /// extractions.
    pub fn from_bound(bound: &Bound<'_, PyAny>) -> PyResult<Self> {
        if bound.is_none() {
            Ok(SerializableValue::None)
        } else if let Ok(b) = bound.downcast::<PyBool>() {
            Ok(SerializableValue::Bool(b.is_true()))
        } else if let Ok(i) = bound.downcast::<PyInt>() {
            match i.extract::<i64>() {
                Ok(v) => Ok(SerializableValue::Int(v)),
                // Out of i64 range: keep the magnitude as a float
                Err(_) => Ok(SerializableValue::Float(i.extract::<f64>()?)),
            }
        } else if let Ok(f) = bound.downcast::<PyFloat>() {
            Ok(SerializableValue::Float(f.value()))
        } else if let Ok(s) = bound.downcast::<PyString>() {
            Ok(SerializableValue::String(s.to_str()?.to_owned()))
        } else if let Ok(list) = bound.downcast::<PyList>() {
            let mut items = Vec::with_capacity(list.len());
            for item in list.iter() {
                items.push(Self::from_bound(&item)?);
            }
            Ok(SerializableValue::List(items))
        } else if let Ok(tuple) = bound.downcast::<PyTuple>() {
            let mut items = Vec::with_capacity(tuple.len());
            for item in tuple.iter() {
                items.push(Self::from_bound(&item)?);
            }
            Ok(SerializableValue::List(items))
        } else if let Ok(dict) = bound.downcast::<PyDict>() {
            let mut map = HashMap::with_capacity(dict.len());
            for (key, value) in dict.iter() {
                map.insert(dict_key(&key)?, Self::from_bound(&value)?);
            }
            Ok(SerializableValue::Dict(map))
        } else if bound.hasattr("tolist")? {
            // numpy arrays and numpy scalars (e.g. embeddings): one bulk
            // conversion to native Python values instead of element-wise
            // access through the sequence protocol.
            Self::from_bound(&bound.call_method0("tolist")?)
        } else if let Ok(mapping) = bound.downcast::<PyMapping>() {
            let mut map = HashMap::new();
            for item in mapping.items()?.iter() {
                let (key, value): (Bound<'_, PyAny>, Bound<'_, PyAny>) = item.extract()?;
                map.insert(dict_key(&key)?, Self::from_bound(&value)?);
            }
            Ok(SerializableValue::Dict(map))
        } else if let Ok(seq) = bound.downcast::<PySequence>() {
            let mut items = Vec::new();
            for item in seq.try_iter()? {
                items.push(Self::from_bound(&item?)?);
            }
            Ok(SerializableValue::List(items))
        } else {
            // Fallback: convert to string representation
            Ok(SerializableValue::String(bound.str()?.extract()?))
        }
    }

    /// Recursively convert Float variants to Half
    pub fn to_f16(&mut self) {
        match self {
            SerializableValue::Float(f) => {
                let half_val = f16::from_f64(*f);
                *self = SerializableValue::Half(half_val);
            }
            SerializableValue::List(list) => {
                for item in list {
                    item.to_f16();
                }
            }
            SerializableValue::Dict(dict) => {
                for value in dict.values_mut() {
                    value.to_f16();
                }
            }
            _ => {}
        }
    }

    /// Convert SerializableValue back to Python object
    pub fn to_python(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match self {
            SerializableValue::None => Ok(py.None()),
            SerializableValue::String(s) => Ok(PyString::new(py, s).into()),
            SerializableValue::Int(i) => Ok(i.into_pyobject(py)?.into_any().into()),
            SerializableValue::Float(f) => Ok(f.into_pyobject(py)?.into_any().into()),
            SerializableValue::Half(h) => Ok(h.to_f64().into_pyobject(py)?.into_any().into()),
            SerializableValue::Bool(b) => {
                let bound = b.into_pyobject(py)?;
                Ok(bound.as_any().clone().into())
            },
            SerializableValue::List(list) => {
                let py_list = pyo3::types::PyList::empty(py);
                for item in list {
                    py_list.append(item.to_python(py)?)?;
                }
                Ok(py_list.into())
            }
            SerializableValue::Dict(dict) => {
                let py_dict = PyDict::new(py);
                for (key, value) in dict {
                    py_dict.set_item(key, value.to_python(py)?)?;
                }
                Ok(py_dict.into())
            }
        }
    }
}

impl SerializableGraph {
    /// Create a SerializableGraph from a Vertex (collection of nodes)
    pub fn from_vertex(py: Python<'_>, vertex: &Vertex) -> PyResult<Self> {
        let mut serializable_nodes = HashMap::new();
        let mut serializable_edges = HashMap::new();
        let mut edge_counter = 0u64;

        // First pass: collect all nodes and their basic info
        for (node_id, node_py) in &vertex.nodes {
            let node = node_py.borrow(py);

            // We'll fill in edge_ids and inverse_edge_ids in the second pass
            let serializable_node = SerializableNode {
                id: node_id.clone(),
                attr: attr_map(py, &node.attr)?,
                meta: attr_map(py, &node.meta)?,
                edge_ids: Vec::new(),
                inverse_edge_ids: Vec::new(),
            };

            serializable_nodes.insert(node_id.clone(), serializable_node);
        }

        // Second pass: collect all edges and update node edge references
        for node_py in vertex.nodes.values() {
            let node = node_py.borrow(py);

            for edge_py in &node.edges {
                let edge = edge_py.borrow(py);

                // Extract edge information
                let from_id = edge.from_node.borrow(py).id.clone();
                let to_id = edge.to_node.borrow(py).id.clone();

                // Generate unique edge ID
                let edge_id = format!("edge_{}_{}_to_{}", edge_counter, from_id, to_id);
                edge_counter += 1;

                // Add edge ID to the source node
                if let Some(node) = serializable_nodes.get_mut(&from_id) {
                    node.edge_ids.push(edge_id.clone());
                }

                // Add edge ID to the target node's inverse_edge_ids
                if let Some(node) = serializable_nodes.get_mut(&to_id) {
                    node.inverse_edge_ids.push(edge_id.clone());
                }

                let serializable_edge = SerializableEdge {
                    id: edge_id.clone(),
                    from_id,
                    to_id,
                    attr: attr_map(py, &edge.attr)?,
                    meta: attr_map(py, &edge.meta)?,
                };

                serializable_edges.insert(edge_id, serializable_edge);
            }
        }

        // Extract vertex meta
        let mut vertex_meta = HashMap::new();
        let meta_dict = vertex.meta.bind(py);
        for (key, value) in meta_dict.iter() {
            let key_str = key.extract::<String>()?;
            vertex_meta.insert(key_str, SerializableValue::from_bound(&value)?);
        }

        // Add some metadata
        let mut metadata = HashMap::new();
        metadata.insert("version".to_string(), SerializableValue::String("1.0".to_string()));
        metadata.insert("node_count".to_string(), SerializableValue::Int(serializable_nodes.len() as i64));
        metadata.insert("edge_count".to_string(), SerializableValue::Int(serializable_edges.len() as i64));
        metadata.insert("timestamp".to_string(), SerializableValue::String(
            chrono::Utc::now().to_rfc3339()
        ));

        Ok(SerializableGraph {
            nodes: serializable_nodes,
            edges: serializable_edges,
            meta: vertex_meta,
            metadata,
        })
    }

    /// Convert SerializableGraph back to a Vertex
    pub fn to_vertex(&self, py: Python<'_>) -> PyResult<Vertex> {
        let mut python_nodes = HashMap::new();
        
        // First pass: create all nodes without edges
        for (node_id, serializable_node) in &self.nodes {
            // Convert attributes back to Python
            let mut python_attr = HashMap::new();
            for (key, value) in &serializable_node.attr {
                python_attr.insert(key.clone(), value.to_python(py)?);
            }
            
            // Convert meta back to Python
            let mut python_meta = HashMap::new();
            for (key, value) in &serializable_node.meta {
                python_meta.insert(key.clone(), value.to_python(py)?);
            }
            
            // Create node with empty edges and inverse_edges for now
            let node = Py::new(py, Node {
                id: serializable_node.id.clone(),
                attr: python_attr,
                meta: python_meta,
                edges: Vec::new(),
                inverse_edges: Vec::new(),
                on_edge_add_callbacks: Vec::new(),
                on_update_callbacks: PyList::empty(py).into(),
                vertex: None,
            })?;
            
            python_nodes.insert(node_id.clone(), node);
        }
        
        // Second pass: create all edges
        let mut python_edges: HashMap<&str, Py<Edge>> = HashMap::with_capacity(self.edges.len());

        for (edge_key, serializable_edge) in &self.edges {
            let from_node = python_nodes.get(&serializable_edge.from_id)
                .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>(
                    format!("From node {} not found", serializable_edge.from_id)
                ))?;
            let to_node = python_nodes.get(&serializable_edge.to_id)
                .ok_or_else(|| PyErr::new::<pyo3::exceptions::PyValueError, _>(
                    format!("To node {} not found", serializable_edge.to_id)
                ))?;

            // Convert edge attributes back to Python
            let mut python_attr = HashMap::new();
            for (key, value) in &serializable_edge.attr {
                python_attr.insert(key.clone(), value.to_python(py)?);
            }

            // Convert edge meta back to Python
            let mut python_meta = HashMap::new();
            for (key, value) in &serializable_edge.meta {
                python_meta.insert(key.clone(), value.to_python(py)?);
            }

            let edge = Py::new(py, Edge {
                id: Some(serializable_edge.id.clone()),
                from_node: from_node.clone_ref(py),
                to_node: to_node.clone_ref(py),
                attr: python_attr,
                meta: python_meta,
                watched_by: Vec::new(),
                on_meta_change_callbacks: Vec::new(),
                on_update_callbacks: PyList::empty(py).into(),
                vertex: None,
            })?;

            python_edges.insert(edge_key.as_str(), edge);
        }

        // Third pass: attach edges to nodes in their saved order (edge_ids /
        // inverse_edge_ids), so edge order survives a save/load round trip.
        let mut attached: HashSet<&str> = HashSet::with_capacity(python_edges.len());
        for (node_id, serializable_node) in &self.nodes {
            let node_py = &python_nodes[node_id];
            let mut node_ref = node_py.bind(py).borrow_mut();
            for edge_id in &serializable_node.edge_ids {
                if let Some(edge) = python_edges.get(edge_id.as_str()) {
                    if self.edges[edge_id].from_id == *node_id {
                        node_ref.edges.push(edge.clone_ref(py));
                        attached.insert(edge_id.as_str());
                    }
                }
            }
            for edge_id in &serializable_node.inverse_edge_ids {
                if let Some(edge) = python_edges.get(edge_id.as_str()) {
                    if self.edges[edge_id].to_id == *node_id {
                        node_ref.inverse_edges.push(edge.clone_ref(py));
                    }
                }
            }
        }

        // Edges not listed in their source node's edge_ids (hand-written or
        // older files) are still attached.
        for (edge_id, edge) in &python_edges {
            if !attached.contains(edge_id) {
                let e = &self.edges[*edge_id];
                python_nodes[&e.from_id].borrow_mut(py).edges.push(edge.clone_ref(py));
                let to = &python_nodes[&e.to_id];
                let already = to.borrow(py).inverse_edges.iter().any(|x| x.is(edge));
                if !already {
                    to.borrow_mut(py).inverse_edges.push(edge.clone_ref(py));
                }
            }
        }

        // Convert vertex meta back to Python
        let vertex_meta_dict = PyDict::new(py);
        for (key, value) in &self.meta {
            vertex_meta_dict.set_item(key, value.to_python(py)?)?;
        }
        
        let mut vertex = Vertex::from_nodes(py, python_nodes);
        vertex.meta = vertex_meta_dict.into();
        Ok(vertex)
    }

    /// Save graph to JSON file
    pub fn save_to_json<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        let mut serializer = serde_json::Serializer::pretty(writer);
        let mut st = serializer.serialize_struct("SerializableGraph", 4)?;
        st.serialize_field("nodes", &self.nodes)?;
        st.serialize_field("edges", &self.edges)?;
        st.serialize_field("meta", &self.meta)?;
        st.serialize_field("metadata", &self.metadata)?;
        st.end()?;
        Ok(())
    }

    /// Serialize graph to JSON string
    pub fn to_json_string(&self) -> Result<String, Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        Ok(json)
    }

    /// Load graph from JSON file
    pub fn load_from_json<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let graph = serde_json::from_reader(reader)?;
        Ok(graph)
    }

    /// Load graph from JSON string
    pub fn from_json_string(json: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let graph = serde_json::from_str(json)?;
        Ok(graph)
    }

    /// Save graph to binary file (more efficient for large graphs)
    pub fn save_to_binary<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let file = File::create(path)?;
        let writer = BufWriter::new(file);
        let options = bincode::DefaultOptions::new().with_fixint_encoding();
        let mut serializer = bincode::Serializer::new(writer, options);
        let mut st = serializer.serialize_struct("SerializableGraph", 4)?;
        st.serialize_field("nodes", &self.nodes)?;
        st.serialize_field("edges", &self.edges)?;
        st.serialize_field("meta", &self.meta)?;
        st.serialize_field("metadata", &self.metadata)?;
        st.end()?;
        Ok(())
    }

    /// Load graph from binary file
    pub fn load_from_binary<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let graph = bincode::deserialize_from(reader)?;
        Ok(graph)
    }

    /// Convert all Float values to Half (f16)
    pub fn convert_floats_to_f16(&mut self) {
        for node in self.nodes.values_mut() {
            for value in node.attr.values_mut() {
                value.to_f16();
            }
            for value in node.meta.values_mut() {
                value.to_f16();
            }
        }
        for edge in self.edges.values_mut() {
            for value in edge.attr.values_mut() {
                value.to_f16();
            }
            for value in edge.meta.values_mut() {
                value.to_f16();
            }
        }
        for value in self.meta.values_mut() {
            value.to_f16();
        }
        for value in self.metadata.values_mut() {
            value.to_f16();
        }
    }

    /// Save graph to binary using f16 for floats
    /// Consumes the graph so the conversion happens in place, without a copy.
    pub fn save_to_binary_f16<P: AsRef<Path>>(mut self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        self.convert_floats_to_f16();
        self.save_to_binary(path)
    }
}

