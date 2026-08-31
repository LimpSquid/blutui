use std::fmt;

use serde::Serialize;

#[derive(Debug, Clone)]
pub enum DiffKind {
    Added {
        path: String,
        value: serde_json::Value,
    },
    Removed {
        path: String,
        value: serde_json::Value,
    },
    Changed {
        path: String,
        old: serde_json::Value,
        new: serde_json::Value,
    },
}

#[derive(Debug, Default)]
pub struct JsonDiff(Vec<DiffKind>);

impl JsonDiff {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for JsonDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for diff in &self.0 {
            match diff {
                DiffKind::Added { path, value } => writeln!(f, "+ {} = {}", path, value)?,
                DiffKind::Removed { path, value } => writeln!(f, "- {} = {}", path, value)?,
                DiffKind::Changed { path, old, new } => {
                    writeln!(f, "~ {}: {} -> {}", path, old, new)?
                }
            }
        }
        Ok(())
    }
}

pub fn json_diff<T: Serialize>(a: &T, b: &T) -> JsonDiff {
    use serde_json::{Value, to_value};

    let a = to_value(a).unwrap();
    let b = to_value(b).unwrap();

    let mut out = Vec::new();
    let mut stack = vec![("".to_string(), &a, &b)];

    while let Some((path, a, b)) = stack.pop() {
        match (a, b) {
            (Value::Object(ao), Value::Object(bo)) => {
                let mut keys: Vec<_> = ao.keys().chain(bo.keys()).collect();
                keys.sort();
                keys.dedup();

                for key in keys.into_iter().rev() {
                    let full_path = if path.is_empty() {
                        key.to_string()
                    } else {
                        format!("{}.{}", path, key)
                    };

                    match (ao.get(key), bo.get(key)) {
                        (Some(av), Some(bv)) => stack.push((full_path, av, bv)),
                        (Some(av), None) => out.push(DiffKind::Removed {
                            path: full_path,
                            value: av.clone(),
                        }),
                        (None, Some(bv)) => out.push(DiffKind::Added {
                            path: full_path,
                            value: bv.clone(),
                        }),
                        _ => {}
                    }
                }
            }
            (Value::Array(aa), Value::Array(ba)) => {
                let max = aa.len().max(ba.len());
                for i in (0..max).rev() {
                    let full_path = format!("{}[{}]", path, i);
                    match (aa.get(i), ba.get(i)) {
                        (Some(av), Some(bv)) => stack.push((full_path, av, bv)),
                        (Some(av), None) => out.push(DiffKind::Removed {
                            path: full_path,
                            value: av.clone(),
                        }),
                        (None, Some(bv)) => out.push(DiffKind::Added {
                            path: full_path,
                            value: bv.clone(),
                        }),
                        _ => {}
                    }
                }
            }
            _ => {
                if a != b {
                    out.push(DiffKind::Changed {
                        path,
                        old: a.clone(),
                        new: b.clone(),
                    });
                }
            }
        }
    }

    JsonDiff(out)
}
