use super::node::{Document, Node, NodeKind};
use super::resolve::{Resolved, resolve};
use super::value::Value;
use super::yaml::{Error, Expected, Got, Issue, IssueKind, PathSegment};
use super::yamlh::{BINARY_TAG, MERGE_TAG, NULL_TAG, STR_TAG};
use crate::gocompat::base64::STD_ENCODING;
use std::collections::HashSet;

const ALIAS_RATIO_RANGE_LOW: u64 = 400_000;
const ALIAS_RATIO_RANGE_HIGH: u64 = 4_000_000;

fn allowed_alias_ratio(decode_count: u64) -> f64 {
    if decode_count <= ALIAS_RATIO_RANGE_LOW {
        0.99
    } else if decode_count >= ALIAS_RATIO_RANGE_HIGH {
        0.10
    } else {
        0.99 - 0.89
            * ((decode_count - ALIAS_RATIO_RANGE_LOW) as f64
                / (ALIAS_RATIO_RANGE_HIGH - ALIAS_RATIO_RANGE_LOW) as f64)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    Mismatch,
    OutOfRange,
}

pub struct Scalar<'a> {
    pub node: &'a Node,
    pub tag: &'a str,
    pub resolved: &'a Resolved,
}

pub trait Target {
    const EXPECTED: Expected;
    const IN_SEQUENCE: Expected = Expected::Array;

    fn decode_scalar(&mut self, scalar: &Scalar<'_>) -> Outcome;

    fn decode_mapping(&mut self, d: &mut Decoder<'_>, n: usize, null: bool) -> Result<bool, Error> {
        let _ = null;
        d.type_error(n, Self::EXPECTED);
        Ok(false)
    }

    fn decode_sequence(
        &mut self,
        d: &mut Decoder<'_>,
        n: usize,
        null: bool,
    ) -> Result<bool, Error> {
        let _ = null;
        d.type_error(n, Self::EXPECTED);
        Ok(false)
    }

    fn scalar_null(&mut self, null: bool) {
        let _ = null;
        self.set_zero();
    }

    fn set_zero(&mut self);

    fn to_value(&self) -> Value;
}

pub trait Fields: Target {
    const KEYS: &'static [&'static str];

    fn decode_field(&mut self, d: &mut Decoder<'_>, index: usize, n: usize) -> Result<bool, Error>;
}

pub struct Decoder<'a> {
    doc: &'a Document,
    strict: bool,
    aliases: HashSet<usize>,
    issues: Vec<Issue>,
    decode_count: u64,
    alias_count: u64,
    alias_depth: u64,
    path: Vec<PathSegment>,
    in_key: bool,
}

fn is_null(node: &Node) -> bool {
    node.tag == NULL_TAG
        || node.kind == NodeKind::Scalar
            && node.tag.is_empty()
            && (node.value == "null" || node.value == "~" || node.value.is_empty() && node.implicit)
}

fn got_of(node: &Node) -> Got {
    match node.kind {
        NodeKind::Mapping => Got::Mapping,
        NodeKind::Sequence => Got::Sequence,
        _ => Got::Scalar(node.value.clone()),
    }
}

impl<'a> Decoder<'a> {
    pub(super) fn new(doc: &'a Document, strict: bool) -> Decoder<'a> {
        Decoder {
            doc,
            strict,
            aliases: HashSet::new(),
            issues: Vec::new(),
            decode_count: 0,
            alias_count: 0,
            alias_depth: 0,
            path: Vec::new(),
            in_key: false,
        }
    }

    pub(super) fn finish(self) -> Result<(), Error> {
        if self.issues.is_empty() {
            Ok(())
        } else {
            Err(Error::Type(self.issues))
        }
    }

    pub fn node(&self, n: usize) -> &'a Node {
        &self.doc.nodes[n]
    }

    fn record(&mut self, n: usize, kind: IssueKind) {
        let line = self.doc.nodes[n].line + 1;
        self.issues.push(Issue {
            line,
            path: self.path.clone(),
            kind,
        });
    }

    pub fn type_error(&mut self, n: usize, expected: Expected) {
        let got = got_of(&self.doc.nodes[n]);
        let kind = if self.in_key {
            IssueKind::Key { got }
        } else {
            IssueKind::Mismatch { expected, got }
        };
        self.record(n, kind);
    }

    pub fn unmarshal<T: Target>(&mut self, n: usize, out: &mut T) -> Result<bool, Error> {
        self.decode_count += 1;
        if self.alias_depth > 0 {
            self.alias_count += 1;
        }
        if self.alias_count > 100
            && self.decode_count > 1000
            && self.alias_count as f64 / self.decode_count as f64
                > allowed_alias_ratio(self.decode_count)
        {
            return Err(Error::Aliasing);
        }
        let doc = self.doc;
        let node = &doc.nodes[n];
        match node.kind {
            NodeKind::Document => {
                if node.children.len() == 1 {
                    self.unmarshal(node.children[0], out)?;
                    return Ok(true);
                }
                Ok(false)
            }
            NodeKind::Alias => self.alias(n, out),
            NodeKind::Scalar => self.scalar(n, out),
            NodeKind::Mapping => out.decode_mapping(self, n, is_null(node)),
            NodeKind::Sequence => out.decode_sequence(self, n, is_null(node)),
        }
    }

    fn alias<T: Target>(&mut self, n: usize, out: &mut T) -> Result<bool, Error> {
        let node = &self.doc.nodes[n];
        if self.aliases.contains(&n) {
            return Err(Error::RecursiveAnchor(node.value.clone()));
        }
        let Some(target) = node.alias else {
            return Err(Error::UnknownAnchor(node.value.clone()));
        };
        self.aliases.insert(n);
        self.alias_depth += 1;
        let good = self.unmarshal(target, out)?;
        self.alias_depth -= 1;
        self.aliases.remove(&n);
        Ok(good)
    }

    fn scalar<T: Target>(&mut self, n: usize, out: &mut T) -> Result<bool, Error> {
        let doc = self.doc;
        let node = &doc.nodes[n];
        let (tag, resolved) = if node.tag.is_empty() && !node.implicit {
            (
                STR_TAG.to_string(),
                Resolved::Str(node.value.as_bytes().to_vec()),
            )
        } else {
            let (tag, resolved) = resolve(&node.tag, &node.value)?;
            if tag == BINARY_TAG {
                let Resolved::Str(text) = &resolved else {
                    return Err(Error::Binary);
                };
                match STD_ENCODING.decode_string(text) {
                    Ok(data) => (tag, Resolved::Str(data)),
                    Err(_) => return Err(Error::Binary),
                }
            } else {
                (tag, resolved)
            }
        };
        if resolved == Resolved::Null {
            out.scalar_null(is_null(node));
            return Ok(true);
        }
        let scalar = Scalar {
            node,
            tag: &tag,
            resolved: &resolved,
        };
        match out.decode_scalar(&scalar) {
            Outcome::Accepted => Ok(true),
            Outcome::Mismatch => {
                self.type_error(n, T::EXPECTED);
                Ok(false)
            }
            Outcome::OutOfRange => {
                let text = node.value.clone();
                self.record(n, IssueKind::OutOfRange { text });
                Ok(false)
            }
        }
    }

    pub fn element<T: Target>(
        &mut self,
        n: usize,
        index: usize,
        out: &mut T,
    ) -> Result<bool, Error> {
        self.path.push(PathSegment::Index(index));
        let good = self.unmarshal(n, out);
        self.path.pop();
        good
    }

    fn is_merge(&self, n: usize) -> bool {
        let node = &self.doc.nodes[n];
        node.kind == NodeKind::Scalar
            && node.value == "<<"
            && (node.implicit || node.tag == MERGE_TAG)
    }

    fn merge<T: Target>(&mut self, n: usize, out: &mut T) -> Result<(), Error> {
        let doc = self.doc;
        let node = &doc.nodes[n];
        match node.kind {
            NodeKind::Mapping => {
                self.unmarshal(n, out)?;
            }
            NodeKind::Alias => {
                if let Some(target) = node.alias
                    && doc.nodes[target].kind != NodeKind::Mapping
                {
                    return Err(Error::Merge);
                }
                self.unmarshal(n, out)?;
            }
            NodeKind::Sequence => {
                for &ni in node.children.iter().rev() {
                    let child = &doc.nodes[ni];
                    if child.kind == NodeKind::Alias {
                        if let Some(target) = child.alias
                            && doc.nodes[target].kind != NodeKind::Mapping
                        {
                            return Err(Error::Merge);
                        }
                    } else if child.kind != NodeKind::Mapping {
                        return Err(Error::Merge);
                    }
                    self.unmarshal(ni, out)?;
                }
            }
            _ => return Err(Error::Merge),
        }
        Ok(())
    }

    pub fn mapping_struct<S: Fields>(&mut self, n: usize, out: &mut S) -> Result<bool, Error> {
        let doc = self.doc;
        let node = &doc.nodes[n];
        let mut done = vec![false; if self.strict { S::KEYS.len() } else { 0 }];
        for pair in node.children.chunks(2) {
            let (ni, vi) = (pair[0], pair[1]);
            if self.is_merge(ni) {
                self.merge(vi, out)?;
                continue;
            }
            let mut name = String::new();
            self.in_key = true;
            let good = self.unmarshal(ni, &mut name);
            self.in_key = false;
            if !good? {
                continue;
            }
            let line = doc.nodes[ni].line + 1;
            match S::KEYS.iter().position(|k| k.as_bytes() == name.as_bytes()) {
                Some(index) => {
                    if self.strict {
                        if done[index] {
                            self.strict_issue(line, name, IssueKind::Duplicate);
                            continue;
                        }
                        done[index] = true;
                    }
                    self.path.push(PathSegment::Key(name));
                    let good = out.decode_field(self, index, vi);
                    self.path.pop();
                    good?;
                }
                None => {
                    if self.strict {
                        self.strict_issue(line, name, IssueKind::Unknown);
                    }
                }
            }
        }
        Ok(true)
    }

    fn strict_issue(&mut self, line: usize, key: String, kind: IssueKind) {
        let mut path = self.path.clone();
        path.push(PathSegment::Key(key));
        self.issues.push(Issue { line, path, kind });
    }

    fn key_text(&self, n: usize) -> String {
        let mut node = &self.doc.nodes[n];
        while let (NodeKind::Alias, Some(target)) = (node.kind, node.alias) {
            node = &self.doc.nodes[target];
        }
        node.value.clone()
    }

    fn mapping_entries(
        &mut self,
        n: usize,
        entries: &mut Vec<(Value, Value)>,
    ) -> Result<bool, Error> {
        let doc = self.doc;
        let node = &doc.nodes[n];
        for pair in node.children.chunks(2) {
            let (ki, vi) = (pair[0], pair[1]);
            if self.is_merge(ki) {
                self.merge(vi, &mut MapInto(entries))?;
                continue;
            }
            let mut key = Value::Null;
            self.unmarshal(ki, &mut key)?;
            match key {
                Value::Map(_) => return Err(Error::MapKey(Got::Mapping)),
                Value::Seq(_) => return Err(Error::MapKey(Got::Sequence)),
                _ => {}
            }
            let text = self.key_text(ki);
            self.path.push(PathSegment::Key(text.clone()));
            let mut value = Value::Null;
            let good = self.unmarshal(vi, &mut value);
            self.path.pop();
            good?;
            let existing = entries.iter().position(|(k, _)| k.go_equal(&key));
            match existing {
                Some(_) if self.strict => {
                    let line = doc.nodes[vi].line + 1;
                    self.strict_issue(line, text, IssueKind::Duplicate);
                }
                Some(index) => entries[index] = (key, value),
                None => entries.push((key, value)),
            }
        }
        Ok(true)
    }
}

struct MapInto<'e>(&'e mut Vec<(Value, Value)>);

impl Target for MapInto<'_> {
    const EXPECTED: Expected = Expected::Mapping;

    fn decode_scalar(&mut self, _scalar: &Scalar<'_>) -> Outcome {
        Outcome::Mismatch
    }

    fn decode_mapping(
        &mut self,
        d: &mut Decoder<'_>,
        n: usize,
        _null: bool,
    ) -> Result<bool, Error> {
        d.mapping_entries(n, self.0)
    }

    fn set_zero(&mut self) {
        self.0.clear();
    }

    fn to_value(&self) -> Value {
        Value::Map(self.0.clone())
    }
}

impl Target for Value {
    const EXPECTED: Expected = Expected::Any;

    fn decode_scalar(&mut self, scalar: &Scalar<'_>) -> Outcome {
        *self = match scalar.resolved {
            Resolved::Null => Value::Null,
            Resolved::Bool(b) => Value::Bool(*b),
            Resolved::Int(v) => Value::Int(*v),
            Resolved::Uint(v) => Value::Uint(*v),
            Resolved::Float(f) => Value::Float(*f),
            Resolved::Str(s) => Value::Str(s.clone()),
            Resolved::Timestamp => Value::Str(scalar.node.value.as_bytes().to_vec()),
        };
        Outcome::Accepted
    }

    fn decode_mapping(
        &mut self,
        d: &mut Decoder<'_>,
        n: usize,
        _null: bool,
    ) -> Result<bool, Error> {
        *self = Value::Map(Vec::new());
        let mut entries = Vec::new();
        let result = d.mapping_entries(n, &mut entries);
        *self = Value::Map(entries);
        result
    }

    fn decode_sequence(
        &mut self,
        d: &mut Decoder<'_>,
        n: usize,
        _null: bool,
    ) -> Result<bool, Error> {
        let mut items: Vec<Value> = Vec::new();
        let result = items.decode_sequence(d, n, false);
        *self = Value::Seq(items);
        result
    }

    fn set_zero(&mut self) {
        *self = Value::Null;
    }

    fn to_value(&self) -> Value {
        self.clone()
    }
}

impl Target for i64 {
    const EXPECTED: Expected = Expected::Integer;

    fn decode_scalar(&mut self, scalar: &Scalar<'_>) -> Outcome {
        match *scalar.resolved {
            Resolved::Int(v) => {
                *self = v;
                Outcome::Accepted
            }
            Resolved::Uint(v) => match i64::try_from(v) {
                Ok(v) => {
                    *self = v;
                    Outcome::Accepted
                }
                Err(_) => Outcome::OutOfRange,
            },
            Resolved::Float(f) => {
                if f <= i64::MAX as f64 {
                    *self = go_float_to_int(f);
                    Outcome::Accepted
                } else if f.is_finite() {
                    Outcome::OutOfRange
                } else {
                    Outcome::Mismatch
                }
            }
            _ => Outcome::Mismatch,
        }
    }

    fn set_zero(&mut self) {
        *self = 0;
    }

    fn to_value(&self) -> Value {
        Value::Int(*self)
    }
}

#[cfg(target_arch = "x86_64")]
fn go_float_to_int(f: f64) -> i64 {
    let min = i64::MIN as f64;
    if f.is_nan() || !(min..-min).contains(&f) {
        i64::MIN
    } else {
        f as i64
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn go_float_to_int(f: f64) -> i64 {
    f as i64
}

impl Target for bool {
    const EXPECTED: Expected = Expected::Boolean;

    fn decode_scalar(&mut self, scalar: &Scalar<'_>) -> Outcome {
        match *scalar.resolved {
            Resolved::Bool(b) => {
                *self = b;
                Outcome::Accepted
            }
            _ => Outcome::Mismatch,
        }
    }

    fn set_zero(&mut self) {
        *self = false;
    }

    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }
}

impl Target for String {
    const EXPECTED: Expected = Expected::String;
    const IN_SEQUENCE: Expected = Expected::Strings;

    fn decode_scalar(&mut self, scalar: &Scalar<'_>) -> Outcome {
        match scalar.resolved {
            Resolved::Str(bytes) if scalar.tag == BINARY_TAG => {
                *self = String::from_utf8_lossy(bytes).into_owned();
            }
            _ => *self = scalar.node.value.clone(),
        }
        Outcome::Accepted
    }

    fn set_zero(&mut self) {
        self.clear();
    }

    fn to_value(&self) -> Value {
        Value::Str(self.as_bytes().to_vec())
    }
}

impl<T: Target + Default> Target for Vec<T> {
    const EXPECTED: Expected = T::IN_SEQUENCE;

    fn decode_scalar(&mut self, _scalar: &Scalar<'_>) -> Outcome {
        Outcome::Mismatch
    }

    fn decode_sequence(
        &mut self,
        d: &mut Decoder<'_>,
        n: usize,
        _null: bool,
    ) -> Result<bool, Error> {
        let node = d.node(n);
        let mut items = Vec::with_capacity(node.children.len());
        for (i, &child) in node.children.iter().enumerate() {
            let mut item = T::default();
            if d.element(child, i, &mut item)? {
                items.push(item);
            }
        }
        *self = items;
        Ok(true)
    }

    fn set_zero(&mut self) {
        self.clear();
    }

    fn to_value(&self) -> Value {
        Value::Seq(self.iter().map(Target::to_value).collect())
    }
}

impl<T: Target + Default> Target for Option<T> {
    const EXPECTED: Expected = T::EXPECTED;

    fn decode_scalar(&mut self, scalar: &Scalar<'_>) -> Outcome {
        self.get_or_insert_with(T::default).decode_scalar(scalar)
    }

    fn decode_mapping(&mut self, d: &mut Decoder<'_>, n: usize, null: bool) -> Result<bool, Error> {
        if null {
            d.type_error(n, T::EXPECTED);
            return Ok(false);
        }
        self.get_or_insert_with(T::default)
            .decode_mapping(d, n, false)
    }

    fn decode_sequence(
        &mut self,
        d: &mut Decoder<'_>,
        n: usize,
        null: bool,
    ) -> Result<bool, Error> {
        if null {
            d.type_error(n, T::EXPECTED);
            return Ok(false);
        }
        self.get_or_insert_with(T::default)
            .decode_sequence(d, n, false)
    }

    fn scalar_null(&mut self, null: bool) {
        if null {
            *self = None;
        } else {
            self.get_or_insert_with(T::default).scalar_null(false);
        }
    }

    fn set_zero(&mut self) {
        *self = None;
    }

    fn to_value(&self) -> Value {
        match self {
            None => Value::Null,
            Some(v) => v.to_value(),
        }
    }
}

impl<T: Target> Target for Box<T> {
    const EXPECTED: Expected = T::EXPECTED;
    const IN_SEQUENCE: Expected = T::IN_SEQUENCE;

    fn decode_scalar(&mut self, scalar: &Scalar<'_>) -> Outcome {
        self.as_mut().decode_scalar(scalar)
    }

    fn decode_mapping(&mut self, d: &mut Decoder<'_>, n: usize, null: bool) -> Result<bool, Error> {
        self.as_mut().decode_mapping(d, n, null)
    }

    fn decode_sequence(
        &mut self,
        d: &mut Decoder<'_>,
        n: usize,
        null: bool,
    ) -> Result<bool, Error> {
        self.as_mut().decode_sequence(d, n, null)
    }

    fn scalar_null(&mut self, null: bool) {
        self.as_mut().scalar_null(null);
    }

    fn set_zero(&mut self) {
        self.as_mut().set_zero();
    }

    fn to_value(&self) -> Value {
        self.as_ref().to_value()
    }
}
