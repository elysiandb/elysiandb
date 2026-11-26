use super::Error;
use super::base64vlq::{self, Reader};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mapping {
    pub gen_line: i32,
    pub gen_column: i32,
    pub sources_ind: i32,
    pub source_line: i32,
    pub source_column: i32,
    pub names_ind: i32,
}

#[derive(Clone, Copy)]
enum Step {
    GenCol,
    SourcesInd,
    SourceLine,
    SourceCol,
    NamesInd,
}

struct Mappings<'a> {
    rd: Reader<'a>,
    has_name: bool,
    value: Mapping,
    values: Vec<Mapping>,
}

pub fn parse_mappings(s: &[u8]) -> Result<Vec<Mapping>, Error> {
    if s.is_empty() {
        return Err(Error::MappingsEmpty);
    }
    let mut m = Mappings {
        rd: Reader::new(s),
        has_name: false,
        value: Mapping {
            gen_line: 1,
            source_line: 1,
            ..Mapping::default()
        },
        values: Vec::with_capacity(mappings_number(s)),
    };
    m.parse()?;
    Ok(m.values)
}

fn mappings_number(s: &[u8]) -> usize {
    s.iter().filter(|&&c| c == b',' || c == b';').count()
}

impl Mappings<'_> {
    fn parse(&mut self) -> Result<(), Error> {
        let mut next = Step::GenCol;
        loop {
            let Some(c) = self.rd.read_byte() else {
                self.push_value();
                return Ok(());
            };
            match c {
                b',' => {
                    self.push_value();
                    next = Step::GenCol;
                }
                b';' => {
                    self.push_value();
                    self.value.gen_line = self.value.gen_line.wrapping_add(1);
                    self.value.gen_column = 0;
                    next = Step::GenCol;
                }
                _ => {
                    self.rd.unread_byte();
                    next = self.step(next)?;
                }
            }
        }
    }

    fn decode(&mut self) -> Result<i32, Error> {
        base64vlq::decode(&mut self.rd).ok_or(Error::UnexpectedEndOfMappings)
    }

    fn step(&mut self, step: Step) -> Result<Step, Error> {
        let n = self.decode()?;
        Ok(match step {
            Step::GenCol => {
                self.value.gen_column = self.value.gen_column.wrapping_add(n);
                Step::SourcesInd
            }
            Step::SourcesInd => {
                self.value.sources_ind = self.value.sources_ind.wrapping_add(n);
                Step::SourceLine
            }
            Step::SourceLine => {
                self.value.source_line = self.value.source_line.wrapping_add(n);
                Step::SourceCol
            }
            Step::SourceCol => {
                self.value.source_column = self.value.source_column.wrapping_add(n);
                Step::NamesInd
            }
            Step::NamesInd => {
                self.has_name = true;
                self.value.names_ind = self.value.names_ind.wrapping_add(n);
                Step::GenCol
            }
        })
    }

    fn push_value(&mut self) {
        if self.value.source_line == 1 && self.value.source_column == 0 {
            return;
        }
        if self.has_name {
            self.values.push(self.value);
            self.has_name = false;
        } else {
            self.values.push(Mapping {
                names_ind: -1,
                ..self.value
            });
        }
    }
}
