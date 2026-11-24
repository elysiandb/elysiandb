#[derive(Clone, Debug, Default, PartialEq)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    Str(Vec<u8>),
    Seq(Vec<Value>),
    Map(Vec<(Value, Value)>),
}

impl Value {
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn go_equal(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Uint(a), Value::Uint(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            _ => false,
        }
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        let mut pending: Vec<Value> = Vec::new();
        match self {
            Value::Seq(items) if !items.is_empty() => pending.append(items),
            Value::Map(entries) if !entries.is_empty() => {
                for (k, v) in entries.drain(..) {
                    pending.push(k);
                    pending.push(v);
                }
            }
            _ => return,
        }
        while let Some(mut v) = pending.pop() {
            match &mut v {
                Value::Seq(items) => pending.append(items),
                Value::Map(entries) => {
                    for (k, val) in entries.drain(..) {
                        pending.push(k);
                        pending.push(val);
                    }
                }
                _ => {}
            }
        }
    }
}
