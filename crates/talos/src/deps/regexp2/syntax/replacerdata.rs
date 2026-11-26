use std::collections::HashMap;

use super::parser::{Error, Parser, RegexOptions};
use super::push_rune;
use super::tree::{NT_CONCATENATE, NT_MULTI, NT_ONE, NT_REF};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplacerData {
    pub rep: String,
    pub strings: Vec<String>,
    pub rules: Vec<isize>,
}

pub(crate) const REPLACE_SPECIALS: isize = 4;

pub fn new_replacer_data(
    rep: &str,
    caps: Option<&HashMap<isize, isize>>,
    capsize: isize,
    capnames: Option<&HashMap<String, isize>>,
    op: RegexOptions,
) -> Result<ReplacerData, Error> {
    let mut p = Parser::new(op);
    p.caps = caps.cloned();
    p.capsize = capsize;
    p.capnames = capnames.cloned();
    p.set_pattern(rep.as_bytes());
    let concat = p.scan_replacement()?;
    if p.arena.nodes[concat].t != NT_CONCATENATE {
        panic!("invalid replacement pattern");
    }
    let mut sb = String::new();
    let mut strings: Vec<String> = Vec::new();
    let mut rules: Vec<isize> = Vec::new();
    let children = p.arena.nodes[concat].children.clone();
    for child in children {
        let node = &p.arena.nodes[child];
        match node.t {
            NT_MULTI => p.arena.write_str_to_buf(child, &mut sb),
            NT_ONE => push_rune(&mut sb, node.ch),
            NT_REF => {
                if !sb.is_empty() {
                    rules.push(strings.len() as isize);
                    strings.push(std::mem::take(&mut sb));
                }
                let mut slot = node.m;
                if let Some(caps) = caps
                    && !caps.is_empty()
                    && slot >= 0
                {
                    slot = caps.get(&slot).copied().unwrap_or(0);
                }
                rules.push(-REPLACE_SPECIALS - 1 - slot);
            }
            _ => panic!("invalid replacement pattern"),
        }
    }
    if !sb.is_empty() {
        rules.push(strings.len() as isize);
        strings.push(sb);
    }
    Ok(ReplacerData {
        rep: rep.to_string(),
        strings,
        rules,
    })
}
