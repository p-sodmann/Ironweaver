//! Validate dense node indices before any algorithm sees them.
use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyMemoryView};

fn checked(value: i64, count: usize) -> PyResult<u32> {
    if value < 0 || value as u64 >= count as u64 {
        Err(PyValueError::new_err(format!("node index {value} is outside 0 .. {count}")))
    } else {
        Ok(value as u32)
    }
}

pub(super) fn pairs(py: Python<'_>, obj: &Bound<'_, PyAny>, count: usize) -> PyResult<Vec<(u32, u32)>> {
    // The buffer path copies native int32/int64 matrices without Python calls
    // per endpoint, including strided numpy arrays.
    let buffered = if let Some((values, shape)) = super::buffer_of::<i64>(py, obj, "pairs", 2)? {
        Some((values, shape))
    } else {
        super::buffer_of::<i32>(py, obj, "pairs", 2)?
            .map(|(values, shape)| (values.into_iter().map(i64::from).collect(), shape))
    };
    if let Some((values, shape)) = buffered {
        if shape[1] != 2 {
            return Err(PyValueError::new_err("pairs must have shape (n, 2)"));
        }
        return values
            .as_chunks::<2>()
            .0
            .iter()
            .map(|row| Ok((checked(row[0], count)?, checked(row[1], count)?)))
            .collect();
    }
    obj.try_iter()?
        .map(|row| {
            let row = row?;
            let values = row.try_iter()?.collect::<PyResult<Vec<_>>>()?;
            if values.len() != 2 {
                return Err(PyValueError::new_err("each pair must contain exactly two node indices"));
            }
            let endpoint = |value: &Bound<'_, PyAny>| {
                // Older NumPy scalar buffers have null shape/stride pointers
                // for zero dimensions, which PyUntypedBuffer rejects. A
                // memoryview still exposes their Boolean format correctly.
                if value.is_instance_of::<PyBool>()
                    || PyMemoryView::from(value)
                        .and_then(|b| b.getattr("format")?.extract::<String>())
                        .is_ok_and(|format| format == "?")
                {
                    return Err(PyTypeError::new_err("node indices must be integers, not bool"));
                }
                let value = value
                    .extract::<i64>()
                    .map_err(|_| PyTypeError::new_err("node indices must be integers in int64 range"))?;
                checked(value, count)
            };
            Ok((endpoint(&values[0])?, endpoint(&values[1])?))
        })
        .collect()
}
