// serialization.rs
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyDict, PyString};
use serde::{Deserialize, Serialize};
use serde::ser::{SerializeStruct, Serializer as _};
use bincode::Options;
use std::collections::HashMap;
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
    Bool(bool),
    None,
    List(Vec<SerializableValue>),
    Dict(HashMap<String, SerializableValue>),
}

/// Complete graph representation for serialization
#[derive(Serialize, Deserialize, Debug)]
pub struct SerializableGraph {
    pub nodes: HashMap<String, SerializableNode>,
    pub edges: HashMap<String, SerializableEdge>,
    pub meta: HashMap<String, SerializableValue>,
    pub metadata: HashMap<String, SerializableValue>,
}

impl SerializableValue {
    /// Convert Python object to SerializableValue
    pub fn from_python(py: Python<'_>, obj: &Py<PyAny>) -> PyResult<Self> {
        let bound = obj.bind(py);
        
        if bound.is_none() {
            Ok(SerializableValue::None)
        } else if let Ok(s) = bound.extract::<String>() {
            Ok(SerializableValue::String(s))
        } else if let Ok(i) = bound.extract::<i64>() {
            Ok(SerializableValue::Int(i))
        } else if let Ok(f) = bound.extract::<f64>() {
            Ok(SerializableValue::Float(f))
        } else if let Ok(b) = bound.extract::<bool>() {
            Ok(SerializableValue::Bool(b))
        } else if let Ok(list) = bound.extract::<Vec<Py<PyAny>>>() {
            let mut serializable_list = Vec::new();
            for item in list {
                serializable_list.push(Self::from_python(py, &item)?);
            }
            Ok(SerializableValue::List(serializable_list))
        } else if bound.hasattr("keys")? {
            // Treat as dictionary
            let mut serializable_dict = HashMap::new();
            let dict = bound.downcast::<pyo3::types::PyDict>()?;
            for (key, value) in dict.iter() {
                let key_str = key.extract::<String>()?;
                let value_py = value.into();
                serializable_dict.insert(key_str, Self::from_python(py, &value_py)?);
            }
            Ok(SerializableValue::Dict(serializable_dict))
        } else {
            // Fallback: convert to string representation
            Ok(SerializableValue::String(bound.str()?.extract()?))
        }
    }

    /// Convert SerializableValue back to Python object
    pub fn to_python(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match self {
            SerializableValue::None => Ok(py.None()),
            SerializableValue::String(s) => Ok(PyString::new(py, s).into()),
            SerializableValue::Int(i) => Ok(i.into_pyobject(py)?.into_any().into()),
            SerializableValue::Float(f) => Ok(f.into_pyobject(py)?.into_any().into()),
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
            let node_ref = node_py.bind(py);
            
            // Extract node attributes
            let attr_py: HashMap<String, Py<PyAny>> = node_ref.getattr("attr")?.extract()?;
            let mut serializable_attr = HashMap::new();
            for (key, value) in attr_py {
                serializable_attr.insert(key, SerializableValue::from_python(py, &value)?);
            }

            // Extract node meta
            let meta_py: HashMap<String, Py<PyAny>> = node_ref.getattr("meta")?.extract()?;
            let mut serializable_meta = HashMap::new();
            for (key, value) in meta_py {
                serializable_meta.insert(key, SerializableValue::from_python(py, &value)?);
            }

            // We'll fill in edge_ids and inverse_edge_ids in the second pass
            let serializable_node = SerializableNode {
                id: node_id.clone(),
                attr: serializable_attr,
                meta: serializable_meta,
                edge_ids: Vec::new(),
                inverse_edge_ids: Vec::new(),
            };
            
            serializable_nodes.insert(node_id.clone(), serializable_node);
        }

        // Second pass: collect all edges and update node edge references
        for (_node_id, node_py) in &vertex.nodes {
            let node_ref = node_py.bind(py);
            let edges: Vec<Py<Edge>> = node_ref.getattr("edges")?.extract()?;
            
            for edge_py in edges {
                let edge_ref = edge_py.bind(py);
                
                // Extract edge information
                let from_node: Py<Node> = edge_ref.getattr("from_node")?.extract()?;
                let to_node: Py<Node> = edge_ref.getattr("to_node")?.extract()?;
                let from_id = from_node.bind(py).getattr("id")?.extract::<String>()?;
                let to_id = to_node.bind(py).getattr("id")?.extract::<String>()?;
                
                // Generate unique edge ID
                let edge_id = format!("edge_{}_{}_to_{}", edge_counter, from_id, to_id);
                edge_counter += 1;
                
                // Extract edge attributes
                let attr_py: HashMap<String, Py<PyAny>> = edge_ref.getattr("attr")?.extract()?;
                let mut serializable_attr = HashMap::new();
                for (key, value) in attr_py {
                    serializable_attr.insert(key, SerializableValue::from_python(py, &value)?);
                }
                
                // Extract edge meta
                let meta_py: HashMap<String, Py<PyAny>> = edge_ref.getattr("meta")?.extract()?;
                let mut serializable_meta = HashMap::new();
                for (key, value) in meta_py {
                    serializable_meta.insert(key, SerializableValue::from_python(py, &value)?);
                }
                
                let serializable_edge = SerializableEdge {
                    id: edge_id.clone(),
                    from_id: from_id.clone(),
                    to_id: to_id.clone(),
                    attr: serializable_attr,
                    meta: serializable_meta,
                };
                
                serializable_edges.insert(edge_id.clone(), serializable_edge);
                
                // Add edge ID to the source node
                if let Some(node) = serializable_nodes.get_mut(&from_id) {
                    node.edge_ids.push(edge_id.clone());
                }
                
                // Add edge ID to the target node's inverse_edge_ids
                if let Some(node) = serializable_nodes.get_mut(&to_id) {
                    node.inverse_edge_ids.push(edge_id);
                }
            }
        }

        // Extract vertex meta
        let mut vertex_meta = HashMap::new();
        let meta_dict = vertex.meta.bind(py);
        for (key, value) in meta_dict.iter() {
            let key_str = key.extract::<String>()?;
            let value_py = value.into();
            vertex_meta.insert(key_str, SerializableValue::from_python(py, &value_py)?);
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
        let mut nodes_map = HashMap::new();
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
            })?;
            
            python_nodes.insert(node_id.clone(), node.clone_ref(py));
            nodes_map.insert(node_id.clone(), node);
        }
        
        // Second pass: create edges and assign them to nodes
        let mut node_edges: HashMap<String, Vec<Py<Edge>>> = HashMap::new();
        let mut node_inverse_edges: HashMap<String, Vec<Py<Edge>>> = HashMap::new();
        
        for serializable_edge in self.edges.values() {
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
            })?;
            
            // Add edge to the from_node's edge list
            node_edges.entry(serializable_edge.from_id.clone())
                .or_insert_with(Vec::new)
                .push(edge.clone_ref(py));
                
            // Add edge to the to_node's inverse_edge list
            node_inverse_edges.entry(serializable_edge.to_id.clone())
                .or_insert_with(Vec::new)
                .push(edge);
        }
        
        // Third pass: update nodes with their edges and inverse_edges
        for (node_id, edges) in node_edges {
            if let Some(node_py) = python_nodes.get(&node_id) {
                let mut node_ref = node_py.bind(py).borrow_mut();
                node_ref.edges = edges;
            }
        }
        
        for (node_id, inverse_edges) in node_inverse_edges {
            if let Some(node_py) = python_nodes.get(&node_id) {
                let mut node_ref = node_py.bind(py).borrow_mut();
                node_ref.inverse_edges = inverse_edges;
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

    /// Load graph from JSON file
    pub fn load_from_json<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let graph = serde_json::from_reader(reader)?;
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
}

// Add chrono for timestamps
use chrono;
