use super::yaml::{Error, SyntaxError, SyntaxKind};
use super::yamlh::{ErrorType, Event, EventType, Parser};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Document,
    Mapping,
    Sequence,
    Scalar,
    Alias,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub line: usize,
    pub column: usize,
    pub tag: String,
    pub value: String,
    pub implicit: bool,
    pub children: Vec<usize>,
    pub alias: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Document {
    pub nodes: Vec<Node>,
    pub anchors: BTreeMap<String, usize>,
}

impl Document {
    pub fn depth(&self) -> usize {
        let mut best = 0;
        let mut stack = vec![(0usize, 1usize)];
        while let Some((n, d)) = stack.pop() {
            best = best.max(d);
            for &c in &self.nodes[n].children {
                stack.push((c, d + 1));
            }
        }
        best
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

struct Builder<'a> {
    parser: Parser<'a>,
    event: Event,
    nodes: Vec<Node>,
    anchors: BTreeMap<String, usize>,
}

impl Builder<'_> {
    fn fail(&self) -> Error {
        let kind = match self.parser.error {
            ErrorType::Reader => SyntaxKind::Reader,
            ErrorType::Scanner => SyntaxKind::Scanner,
            _ => SyntaxKind::Parser,
        };
        let problem = if self.parser.problem.is_empty() {
            "unknown problem parsing YAML content".to_string()
        } else {
            self.parser.problem.clone()
        };
        Error::Syntax(SyntaxError {
            kind,
            problem,
            context: self.parser.context,
            problem_mark: self.parser.problem_mark,
            context_mark: self.parser.context_mark,
            problem_offset: self.parser.problem_offset,
            problem_value: self.parser.problem_value,
        })
    }

    fn peek(&mut self) -> Result<EventType, Error> {
        if self.event.typ != EventType::NoEvent {
            return Ok(self.event.typ);
        }
        match self.parser.parse() {
            Some(e) => {
                self.event = e;
                Ok(self.event.typ)
            }
            None => Err(self.fail()),
        }
    }

    fn expect(&mut self, typ: EventType) -> Result<(), Error> {
        self.peek()?;
        if self.event.typ != typ {
            self.parser.problem = format!("expected {typ:?} event but got {:?}", self.event.typ);
            self.parser.problem_mark = Default::default();
            self.parser.context_mark = Default::default();
            return Err(self.fail());
        }
        self.event = Event::default();
        Ok(())
    }

    fn node(&mut self, kind: NodeKind) -> usize {
        let node = Node {
            kind,
            line: self.event.start_mark.line,
            column: self.event.start_mark.column,
            tag: String::new(),
            value: String::new(),
            implicit: false,
            children: Vec::new(),
            alias: None,
        };
        self.nodes.push(node);
        let id = self.nodes.len() - 1;
        if !self.event.anchor.is_empty() && kind != NodeKind::Alias && kind != NodeKind::Document {
            self.anchors.insert(text(&self.event.anchor), id);
        }
        id
    }

    fn document(&mut self) -> Result<(), Error> {
        let doc = self.node(NodeKind::Document);
        self.expect(EventType::DocumentStart)?;
        let mut stack: Vec<usize> = Vec::new();
        loop {
            let typ = self.peek()?;
            let id = match typ {
                EventType::Scalar => {
                    let id = self.node(NodeKind::Scalar);
                    self.nodes[id].value = text(&self.event.value);
                    self.nodes[id].tag = text(&self.event.tag);
                    self.nodes[id].implicit = self.event.implicit;
                    self.expect(EventType::Scalar)?;
                    id
                }
                EventType::Alias => {
                    let name = text(&self.event.anchor);
                    let target = match self.anchors.get(&name) {
                        Some(&t) => t,
                        None => return Err(Error::UnknownAnchor(name)),
                    };
                    let id = self.node(NodeKind::Alias);
                    self.nodes[id].value = name;
                    self.nodes[id].alias = Some(target);
                    self.expect(EventType::Alias)?;
                    id
                }
                EventType::SequenceStart | EventType::MappingStart => {
                    let kind = if typ == EventType::SequenceStart {
                        NodeKind::Sequence
                    } else {
                        NodeKind::Mapping
                    };
                    let id = self.node(kind);
                    self.expect(typ)?;
                    match stack.last() {
                        Some(&parent) => self.nodes[parent].children.push(id),
                        None => self.nodes[doc].children.push(id),
                    }
                    stack.push(id);
                    continue;
                }
                EventType::SequenceEnd | EventType::MappingEnd if !stack.is_empty() => {
                    self.expect(typ)?;
                    stack.pop();
                    if stack.is_empty() {
                        break;
                    }
                    continue;
                }
                other => {
                    self.parser.problem = format!("attempted to parse unknown event: {other:?}");
                    self.parser.problem_mark = Default::default();
                    self.parser.context_mark = Default::default();
                    return Err(self.fail());
                }
            };
            match stack.last() {
                Some(&parent) => self.nodes[parent].children.push(id),
                None => {
                    self.nodes[doc].children.push(id);
                    break;
                }
            }
        }
        self.expect(EventType::DocumentEnd)
    }
}

pub fn parse(data: &[u8]) -> Result<Option<Document>, Error> {
    let input: &[u8] = if data.is_empty() { b"\n" } else { data };
    let mut b = Builder {
        parser: Parser::new(input),
        event: Event::default(),
        nodes: Vec::new(),
        anchors: BTreeMap::new(),
    };
    b.expect(EventType::StreamStart)?;
    match b.peek()? {
        EventType::DocumentStart => {
            b.document()?;
            Ok(Some(Document {
                nodes: b.nodes,
                anchors: b.anchors,
            }))
        }
        _ => Ok(None),
    }
}
