//! Rust fast paths for exact built-in containers. A shared Python memo also
//! lets custom copying hooks participate in cycles and shared references.

use pyo3::prelude::*;
use pyo3::types::{PyBool, PyBytes, PyComplex, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};

pub(super) struct DeepCopy<'py> {
    memo: Bound<'py, PyDict>,
    fallback: Bound<'py, PyAny>,
}

impl<'py> DeepCopy<'py> {
    pub fn new(py: Python<'py>) -> PyResult<Self> {
        Ok(Self { memo: PyDict::new(py), fallback: py.import("copy")?.getattr("deepcopy")? })
    }

    pub fn copy(&self, value: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
        self.copy_inner(value, 0)
    }

    fn remember(&self, value: &Bound<'py, PyAny>, copied: &Bound<'py, PyAny>) -> PyResult<()> {
        self.memo.set_item(value.as_ptr() as usize, copied)?;
        // Match copy._keep_alive: source identities cannot be reused while
        // custom hooks are running. The fallback uses this same memo entry.
        let key = self.memo.as_ptr() as usize;
        if let Some(keep) = self.memo.get_item(key)? {
            keep.cast::<PyList>()?.append(value)?;
        } else {
            self.memo.set_item(key, PyList::new(value.py(), [value])?)?;
        }
        Ok(())
    }

    fn copy_inner(&self, value: &Bound<'py, PyAny>, depth: usize) -> PyResult<Bound<'py, PyAny>> {
        let py = value.py();
        py.check_signals()?;
        // Only exact built-ins: subclasses may define their own copying hooks.
        if value.is_none()
            || value.cast_exact::<PyBool>().is_ok()
            || value.cast_exact::<PyInt>().is_ok()
            || value.cast_exact::<PyFloat>().is_ok()
            || value.cast_exact::<PyComplex>().is_ok()
            || value.cast_exact::<PyString>().is_ok()
            || value.cast_exact::<PyBytes>().is_ok()
        {
            return Ok(value.clone());
        }
        let id = value.as_ptr() as usize;
        if let Some(copied) = self.memo.get_item(id)? {
            return Ok(copied);
        }
        // Bound native stack use; Python handles deeper values with its usual
        // recursion checks, and propagates RecursionError when necessary.
        if depth >= 100 {
            return self.fallback.call1((value, &self.memo));
        }
        if let Ok(list) = value.cast_exact::<PyList>() {
            let out = PyList::empty(py);
            self.remember(value, out.as_any())?;
            for item in list.iter() {
                out.append(self.copy_inner(&item, depth + 1)?)?;
            }
            Ok(out.into_any())
        } else if let Ok(dict) = value.cast_exact::<PyDict>() {
            let out = PyDict::new(py);
            self.remember(value, out.as_any())?;
            // Snapshot entries: a custom hook can mutate the source dict.
            for entry in dict.items().iter() {
                let entry = entry.cast::<PyTuple>()?;
                let item = self.copy_inner(&entry.get_item(1)?, depth + 1)?;
                let key = self.copy_inner(&entry.get_item(0)?, depth + 1)?;
                out.set_item(key, item)?;
            }
            Ok(out.into_any())
        } else if let Ok(tuple) = value.cast_exact::<PyTuple>() {
            let items: PyResult<Vec<_>> = tuple.iter().map(|v| self.copy_inner(&v, depth + 1)).collect();
            let items = items?;
            // A tuple -> list -> tuple cycle may already have copied this tuple.
            if let Some(copied) = self.memo.get_item(id)? {
                return Ok(copied);
            }
            if tuple.iter().zip(&items).all(|(a, b)| a.is(b)) {
                return Ok(value.clone());
            }
            let out = PyTuple::new(py, items)?;
            self.remember(value, out.as_any())?;
            Ok(out.into_any())
        } else {
            self.fallback.call1((value, &self.memo))
        }
    }
}
