use std::cell::OnceCell;
use std::ops::Deref;
use std::sync::Arc;

use gocompat::utf8::Rune;

use super::regexp::Regexp;
use super::syntax::{push_rune, runes_to_string};

#[derive(Clone, Debug)]
pub struct Capture {
    pub(crate) text: Arc<[Rune]>,
    pub index: isize,
    pub length: isize,
}

impl Capture {
    pub fn string(&self) -> String {
        runes_to_string(self.runes())
    }

    pub fn runes(&self) -> &[Rune] {
        &self.text[self.index as usize..(self.index + self.length) as usize]
    }
}

#[derive(Clone, Debug)]
pub struct Group {
    pub capture: Capture,
    pub name: String,
    pub captures: Vec<Capture>,
}

impl Deref for Group {
    type Target = Capture;

    fn deref(&self) -> &Capture {
        &self.capture
    }
}

#[derive(Clone, Debug)]
pub(crate) struct MatchState {
    pub(crate) matchcount: Vec<isize>,
    pub(crate) matches: Vec<Option<Vec<isize>>>,
    pub(crate) balancing: bool,
}

impl MatchState {
    pub(crate) fn new(capcount: isize) -> MatchState {
        let n = capcount as usize;
        let mut matches = vec![None; n];
        matches[0] = Some(vec![0; 2]);
        MatchState {
            matchcount: vec![0; n],
            matches,
            balancing: false,
        }
    }

    pub(crate) fn reset(&mut self) {
        for c in &mut self.matchcount {
            *c = 0;
        }
        self.balancing = false;
    }

    fn slots(&self, cap: isize) -> &Vec<isize> {
        self.matches[cap as usize]
            .as_ref()
            .expect("a matched group has slots")
    }

    pub(crate) fn is_matched(&self, cap: isize) -> bool {
        cap < self.matchcount.len() as isize
            && self.matchcount[cap as usize] > 0
            && self.slots(cap)[(self.matchcount[cap as usize] * 2 - 1) as usize] != -3 + 1
    }

    pub(crate) fn match_index(&self, cap: isize) -> isize {
        let slots = self.slots(cap);
        let i = slots[(self.matchcount[cap as usize] * 2 - 2) as usize];
        if i >= 0 {
            return i;
        }
        slots[(-3 - i) as usize]
    }

    pub(crate) fn match_length(&self, cap: isize) -> isize {
        let slots = self.slots(cap);
        let i = slots[(self.matchcount[cap as usize] * 2 - 1) as usize];
        if i >= 0 {
            return i;
        }
        slots[(-3 - i) as usize]
    }

    pub(crate) fn add_match(&mut self, c: isize, start: isize, l: isize) {
        let cu = c as usize;
        if self.matches[cu].is_none() {
            self.matches[cu] = Some(vec![0; 2]);
        }
        let capcount = self.matchcount[cu];
        let slots = self.matches[cu].as_mut().expect("group slots");
        if (capcount * 2 + 2) as usize > slots.len() {
            let mut newmatches = vec![0; (capcount * 8) as usize];
            newmatches[..(capcount * 2) as usize]
                .copy_from_slice(&slots[..(capcount * 2) as usize]);
            *slots = newmatches;
        }
        slots[(capcount * 2) as usize] = start;
        slots[(capcount * 2 + 1) as usize] = l;
        self.matchcount[cu] = capcount + 1;
    }

    pub(crate) fn balance_match(&mut self, c: isize) {
        self.balancing = true;
        let capcount = self.matchcount[c as usize];
        let mut target = capcount * 2 - 2;
        let slots = self.slots(c);
        if slots[target as usize] < 0 {
            target = -3 - slots[target as usize];
        }
        target -= 2;
        if target >= 0 && slots[target as usize] < 0 {
            let (a, b) = (slots[target as usize], slots[(target + 1) as usize]);
            self.add_match(c, a, b);
        } else {
            self.add_match(c, -3 - target, -4 - target);
        }
    }

    pub(crate) fn remove_match(&mut self, c: isize) {
        self.matchcount[c as usize] -= 1;
    }

    fn tidy(&mut self) -> (isize, isize) {
        let interval = self.matches[0].as_ref().expect("group 0 slots");
        let index = interval[0];
        let length = interval[1];
        if self.balancing {
            for cap in 0..self.matchcount.len() {
                let limit = (self.matchcount[cap] * 2) as usize;
                let Some(matcharray) = self.matches[cap].as_mut() else {
                    continue;
                };
                let mut i = 0usize;
                while i < limit {
                    if matcharray[i] < 0 {
                        break;
                    }
                    i += 1;
                }
                let mut j = i as isize;
                while i < limit {
                    if matcharray[i] < 0 {
                        j -= 1;
                    } else {
                        if i as isize != j {
                            matcharray[j as usize] = matcharray[i];
                        }
                        j += 1;
                    }
                    i += 1;
                }
                self.matchcount[cap] = j / 2;
            }
            self.balancing = false;
        }
        (index, length)
    }
}

#[derive(Debug)]
pub struct Match<'r> {
    pub group: Group,
    regex: &'r Regexp,
    other_groups: OnceCell<Vec<Group>>,
    pub(crate) textpos: isize,
    matches: Vec<Option<Vec<isize>>>,
    matchcount: Vec<isize>,
}

impl Deref for Match<'_> {
    type Target = Group;

    fn deref(&self) -> &Group {
        &self.group
    }
}

impl<'r> Match<'r> {
    pub(crate) fn from_state(
        regex: &'r Regexp,
        mut state: MatchState,
        text: Arc<[Rune]>,
        textpos: isize,
    ) -> Match<'r> {
        let (index, length) = state.tidy();
        let capture = Capture {
            text,
            index,
            length,
        };
        Match {
            group: Group {
                capture: capture.clone(),
                name: "0".to_string(),
                captures: vec![capture],
            },
            regex,
            other_groups: OnceCell::new(),
            textpos,
            matches: state.matches,
            matchcount: state.matchcount,
        }
    }

    pub(crate) fn text(&self) -> &Arc<[Rune]> {
        &self.group.capture.text
    }

    pub fn group_count(&self) -> usize {
        self.matchcount.len()
    }

    pub fn group_by_name(&self, name: &str) -> Option<&Group> {
        let num = self.regex.group_number_from_name(name);
        if num < 0 {
            return None;
        }
        self.group_by_number(num)
    }

    pub fn group_by_number(&self, num: isize) -> Option<&Group> {
        let mut num = num;
        if let Some(caps) = &self.regex.caps
            && let Some(&new_num) = caps.get(&num)
        {
            num = new_num;
        }
        if num >= self.matchcount.len() as isize || num < 0 {
            return None;
        }
        if num == 0 {
            return Some(&self.group);
        }
        Some(&self.other_groups()[(num - 1) as usize])
    }

    pub fn groups(&self) -> Vec<&Group> {
        let mut g = Vec::with_capacity(self.matchcount.len());
        g.push(&self.group);
        g.extend(self.other_groups().iter());
        g
    }

    fn other_groups(&self) -> &Vec<Group> {
        self.other_groups.get_or_init(|| {
            (1..self.matchcount.len())
                .map(|i| {
                    new_group(
                        self.regex.group_name_from_number(i as isize),
                        self.text(),
                        self.matches[i].as_deref(),
                        self.matchcount[i],
                    )
                })
                .collect()
        })
    }

    pub(crate) fn group_value_append_to_buf(&self, groupnum: isize, buf: &mut String) {
        let c = self.matchcount[groupnum as usize];
        if c == 0 {
            return;
        }
        let matches = self.matches[groupnum as usize]
            .as_ref()
            .expect("a matched group has slots");
        let mut index = matches[((c - 1) * 2) as usize];
        let last = index + matches[(c * 2 - 1) as usize];
        let text = self.text();
        while index < last {
            push_rune(buf, text[index as usize]);
            index += 1;
        }
    }

    pub fn dump(&self) -> String {
        let mut buf = String::from("\n");
        if let Some(caps) = &self.regex.caps {
            let mut pairs: Vec<(&isize, &isize)> = caps.iter().collect();
            pairs.sort();
            for (k, v) in pairs {
                buf.push_str(&format!("Slot {k} -> {v}\n"));
            }
        }
        for (i, g) in self.groups().iter().enumerate() {
            buf.push_str(&format!(
                "Group {} ({}), {} caps:\n",
                i,
                g.name,
                g.captures.len()
            ));
            for c in &g.captures {
                buf.push_str(&format!("  ({}, {}) {}\n", c.index, c.length, c.string()));
            }
        }
        buf
    }
}

fn new_group(name: String, text: &Arc<[Rune]>, caps: Option<&[isize]>, capcount: isize) -> Group {
    let mut capture = Capture {
        text: text.clone(),
        index: 0,
        length: 0,
    };
    let caps = caps.unwrap_or(&[]);
    if capcount > 0 {
        capture.index = caps[((capcount - 1) * 2) as usize];
        capture.length = caps[(capcount * 2 - 1) as usize];
    }
    let captures = (0..capcount as usize)
        .map(|i| Capture {
            text: text.clone(),
            index: caps[i * 2],
            length: caps[i * 2 + 1],
        })
        .collect();
    Group {
        capture,
        name,
        captures,
    }
}
