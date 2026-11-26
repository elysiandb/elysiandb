use super::Error;
use super::decode::{
    Cursor, GoSlice, Kind, decode_int, decode_slice, decode_string, field_path, match_field,
    type_error,
};
use super::mappings::{Mapping, parse_mappings};
use super::{path, url};

const STRING_SIZE: usize = 16;
const RAW_MESSAGE_SIZE: usize = 24;
const SECTION_SIZE: usize = 24;

#[derive(Clone, Debug, Default)]
struct SourceMapData {
    version: i64,
    file: Vec<u8>,
    source_root: Vec<u8>,
    sources: GoSlice<Vec<u8>>,
    sources_content: GoSlice<Vec<u8>>,
    names: GoSlice<Vec<u8>>,
    mappings: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
struct Offset {
    line: i64,
    column: i64,
}

#[derive(Clone, Debug, Default)]
struct SectionData {
    offset: Offset,
    map: Option<Box<SourceMapData>>,
}

#[derive(Clone, Debug, Default)]
struct V3 {
    source_map: SourceMapData,
    sections: GoSlice<SectionData>,
}

#[derive(Clone, Debug)]
struct SourceMap {
    sources: Vec<String>,
    sources_content: Vec<Vec<u8>>,
    names: Vec<Vec<u8>>,
    mappings: Vec<Mapping>,
}

#[derive(Clone, Debug)]
struct Section {
    offset: Offset,
    map: SourceMap,
}

#[derive(Clone, Debug)]
pub struct Consumer {
    sourcemap_url: String,
    file: String,
    sections: Vec<Section>,
}

const SOURCE_MAP_FIELDS: [&str; 7] = [
    "version",
    "file",
    "sourceRoot",
    "sources",
    "sourcesContent",
    "names",
    "mappings",
];

const V3_FIELDS: [&str; 8] = [
    "version",
    "file",
    "sourceRoot",
    "sources",
    "sourcesContent",
    "names",
    "mappings",
    "sections",
];

fn decode_source_map_field(
    c: &mut Cursor<'_>,
    name: &str,
    path: &str,
    m: &mut SourceMapData,
) -> Result<(), Error> {
    match name {
        "version" => decode_int(c, path, &mut m.version),
        "file" => decode_string(c, path, &mut m.file),
        "sourceRoot" => decode_string(c, path, &mut m.source_root),
        "sources" => decode_slice(
            c,
            path,
            "an array of strings",
            STRING_SIZE,
            &mut m.sources,
            decode_string,
        ),
        "sourcesContent" => decode_slice(
            c,
            path,
            "an array of strings",
            STRING_SIZE,
            &mut m.sources_content,
            decode_string,
        ),
        "names" => decode_slice(
            c,
            path,
            "an array",
            RAW_MESSAGE_SIZE,
            &mut m.names,
            |c, _, t| {
                *t = c.skip_value().to_vec();
                Ok(())
            },
        ),
        _ => decode_string(c, path, &mut m.mappings),
    }
}

fn decode_source_map(c: &mut Cursor<'_>, path: &str, m: &mut SourceMapData) -> Result<(), Error> {
    c.object(|c, key| {
        let p = field_path(path, key);
        match match_field(key, &SOURCE_MAP_FIELDS) {
            Some(name) => decode_source_map_field(c, name, &p, m),
            None => {
                c.skip_value();
                Ok(())
            }
        }
    })
}

fn decode_offset(c: &mut Cursor<'_>, path: &str, o: &mut Offset) -> Result<(), Error> {
    let kind = c.kind();
    match kind {
        Kind::Null => {
            c.skip_value();
            Ok(())
        }
        Kind::Object => c.object(|c, key| {
            let p = field_path(path, key);
            match match_field(key, &["line", "column"]) {
                Some("line") => decode_int(c, &p, &mut o.line),
                Some(_) => decode_int(c, &p, &mut o.column),
                None => {
                    c.skip_value();
                    Ok(())
                }
            }
        }),
        _ => Err(type_error(path, "an object", kind)),
    }
}

fn decode_section_map(
    c: &mut Cursor<'_>,
    path: &str,
    m: &mut Option<Box<SourceMapData>>,
) -> Result<(), Error> {
    let kind = c.kind();
    match kind {
        Kind::Null => {
            c.skip_value();
            *m = None;
            Ok(())
        }
        Kind::Object => decode_source_map(c, path, m.get_or_insert_with(Box::default)),
        _ => Err(type_error(path, "an object", kind)),
    }
}

fn decode_section(c: &mut Cursor<'_>, path: &str, s: &mut SectionData) -> Result<(), Error> {
    let kind = c.kind();
    match kind {
        Kind::Null => {
            c.skip_value();
            Ok(())
        }
        Kind::Object => c.object(|c, key| {
            let p = field_path(path, key);
            match match_field(key, &["offset", "map"]) {
                Some("offset") => decode_offset(c, &p, &mut s.offset),
                Some(_) => decode_section_map(c, &p, &mut s.map),
                None => {
                    c.skip_value();
                    Ok(())
                }
            }
        }),
        _ => Err(type_error(path, "an object", kind)),
    }
}

fn unmarshal(b: &[u8]) -> Result<V3, Error> {
    let mut c = Cursor::new(b)?;
    let mut v = V3::default();
    let kind = c.kind();
    match kind {
        Kind::Null => {}
        Kind::Object => c.object(|c, key| {
            let p = field_path("", key);
            match match_field(key, &V3_FIELDS) {
                Some("sections") => decode_slice(
                    c,
                    &p,
                    "an array",
                    SECTION_SIZE,
                    &mut v.sections,
                    decode_section,
                ),
                Some(name) => decode_source_map_field(c, name, &p, &mut v.source_map),
                None => {
                    c.skip_value();
                    Ok(())
                }
            }
        })?,
        _ => return Err(type_error("", "an object", kind)),
    }
    Ok(v)
}

fn check_version(version: i64) -> Result<(), Error> {
    if version == 3 || version == 0 {
        return Ok(());
    }
    Err(Error::Version(version))
}

fn bytes_string(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

impl SourceMapData {
    fn parse(self, sourcemap_url: &[u8]) -> Result<SourceMap, Error> {
        check_version(self.version)?;
        let mut source_root_url: Option<url::Url> = None;
        if !self.source_root.is_empty() {
            let u = url::parse(&self.source_root).map_err(Error::Url)?;
            if u.is_abs() {
                source_root_url = Some(u);
            }
        } else if !sourcemap_url.is_empty() {
            let mut u = url::parse(sourcemap_url).map_err(Error::Url)?;
            if u.is_abs() {
                u.path = path::dir(&u.path);
                source_root_url = Some(u);
            }
        }
        let sources: Vec<String> = self
            .sources
            .as_slice()
            .iter()
            .map(|src| {
                bytes_string(&abs_source(
                    &self.source_root,
                    source_root_url.as_ref(),
                    src,
                ))
            })
            .collect();
        let mappings = parse_mappings(&self.mappings)?;
        Ok(SourceMap {
            sources,
            sources_content: self.sources_content.into_vec(),
            names: self.names.into_vec(),
            mappings,
        })
    }
}

fn abs_source(source_root: &[u8], root: Option<&url::Url>, source: &[u8]) -> Vec<u8> {
    if path::is_abs(source) {
        return source.to_vec();
    }
    if let Ok(u) = url::parse(source)
        && u.is_abs()
    {
        return source.to_vec();
    }
    if let Some(root) = root {
        let mut u = root.clone();
        u.path = path::join(&[&u.path, source]);
        return u.string_bytes();
    }
    if !source_root.is_empty() {
        return path::join(&[source_root, source]);
    }
    source.to_vec()
}

impl SourceMap {
    fn name(&self, idx: usize) -> String {
        if idx >= self.names.len() {
            return String::new();
        }
        let raw = &self.names[idx];
        if raw.is_empty() {
            return String::new();
        }
        if raw[0] == b'"'
            && raw[raw.len() - 1] == b'"'
            && let Some(s) = gocompat::json::unquote_bytes(raw)
        {
            return bytes_string(&s);
        }
        bytes_string(raw)
    }

    fn source(&self, gen_line: i64, gen_column: i64) -> (String, String, i64, i64, bool) {
        let i = search(self.mappings.len(), |i| {
            let m = &self.mappings[i];
            if i64::from(m.gen_line) == gen_line {
                return i64::from(m.gen_column) >= gen_column;
            }
            i64::from(m.gen_line) >= gen_line
        });
        if i == self.mappings.len() {
            return (String::new(), String::new(), 0, 0, false);
        }
        let mut matched = &self.mappings[i];
        if i64::from(matched.gen_line) > gen_line || i64::from(matched.gen_column) > gen_column {
            if i == 0 {
                return (String::new(), String::new(), 0, 0, false);
            }
            matched = &self.mappings[i - 1];
        }
        let mut source = String::new();
        let mut name = String::new();
        if matched.sources_ind >= 0 {
            source = self.sources[matched.sources_ind as usize].clone();
        }
        if matched.names_ind >= 0 {
            name = self.name(matched.names_ind as usize);
        }
        (
            source,
            name,
            i64::from(matched.source_line),
            i64::from(matched.source_column),
            true,
        )
    }
}

fn search(n: usize, mut f: impl FnMut(usize) -> bool) -> usize {
    let (mut i, mut j) = (0usize, n);
    while i < j {
        let h = (i + j) >> 1;
        if !f(h) {
            i = h + 1;
        } else {
            j = h;
        }
    }
    i
}

pub fn parse(sourcemap_url: &str, b: &[u8]) -> Result<Consumer, Error> {
    let v3 = unmarshal(b)?;
    check_version(v3.source_map.version)?;
    let file = bytes_string(&v3.source_map.file);
    let mut sections: Vec<SectionData> = v3.sections.into_vec();
    if sections.is_empty() {
        sections.push(SectionData {
            offset: Offset::default(),
            map: Some(Box::new(v3.source_map)),
        });
    }
    let mut parsed = Vec::with_capacity(sections.len());
    for s in sections {
        let Some(map) = s.map else {
            panic!("source map section without a map");
        };
        parsed.push(Section {
            offset: s.offset,
            map: map.parse(sourcemap_url.as_bytes())?,
        });
    }
    parsed.reverse();
    Ok(Consumer {
        sourcemap_url: sourcemap_url.to_string(),
        file,
        sections: parsed,
    })
}

impl Consumer {
    pub fn sourcemap_url(&self) -> &str {
        &self.sourcemap_url
    }

    pub fn file(&self) -> &str {
        &self.file
    }

    pub fn source(&self, gen_line: i64, gen_column: i64) -> (String, String, i64, i64, bool) {
        for s in &self.sections {
            if s.offset.line < gen_line
                || (s.offset.line.wrapping_add(1) == gen_line && s.offset.column <= gen_column)
            {
                return s.map.source(
                    gen_line.wrapping_sub(s.offset.line),
                    gen_column.wrapping_sub(s.offset.column),
                );
            }
        }
        (String::new(), String::new(), 0, 0, false)
    }

    pub fn source_content(&self, source: &str) -> String {
        for s in &self.sections {
            for (i, src) in s.map.sources.iter().enumerate() {
                if src == source {
                    if i < s.map.sources_content.len() {
                        return bytes_string(&s.map.sources_content[i]);
                    }
                    break;
                }
            }
        }
        String::new()
    }
}
