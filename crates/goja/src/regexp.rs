use crate::builtin_string::{StrUnits, lenient_decode_at};
use crate::builtin_symbol::{SYM_MATCH, SYM_MATCH_ALL, SYM_REPLACE, SYM_SEARCH, SYM_SPLIT};
use crate::deps::regexp;
use crate::deps::regexp2;
use crate::heap::{ObjRef, SymRef, Tracer};
use crate::object::{BASE_CLASS, BaseObject, ObjClass, ObjData, PropertyDescriptor, UStr};
use crate::runtime::{JsResult, Runtime, to_length};
use crate::string::{JsString, devirtualize_string};
use crate::string_unicode::UnicodeString;
use crate::value::{Value, int_to_value, nil_safe};
use gocompat::utf8;
use gocompat::utf16;
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, LazyLock};

pub(crate) static LAST_INDEX: LazyLock<UStr> = LazyLock::new(|| UStr::new("lastIndex"));
static INPUT: LazyLock<UStr> = LazyLock::new(|| UStr::new("input"));
static INDEX: LazyLock<UStr> = LazyLock::new(|| UStr::new("index"));
static EXEC: LazyLock<UStr> = LazyLock::new(|| UStr::new("exec"));

#[derive(Clone, Debug)]
pub struct Regexp2MatchCache {
    pub target: JsString,
    pub runes: Arc<[i32]>,
    pub pos_map: Option<Arc<[isize]>>,
}

#[derive(Debug)]
pub struct Regexp2Wrapper {
    pub rx: Arc<regexp2::Regexp>,
    pub cache: Option<Regexp2MatchCache>,
}

impl Clone for Regexp2Wrapper {
    fn clone(&self) -> Regexp2Wrapper {
        Regexp2Wrapper {
            rx: self.rx.clone(),
            cache: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RegexpWrapper(pub regexp::Regexp);

#[derive(Clone, Copy)]
struct PositionMapItem {
    src: isize,
    dst: isize,
}

struct PositionMap(Vec<PositionMapItem>);

impl PositionMap {
    fn get(&self, src: isize) -> isize {
        if src <= 0 {
            return src;
        }
        let res = self.0.partition_point(|m| m.src < src);
        if res >= self.0.len() || self.0[res].src != src {
            panic!("index not found");
        }
        self.0[res].dst
    }
}

struct ArrayRuneReader<'a> {
    runes: &'a [i32],
    pos: usize,
}

impl regexp::RuneReader for ArrayRuneReader<'_> {
    fn read_rune(&mut self) -> Option<(i32, usize)> {
        if self.pos < self.runes.len() {
            let r = self.runes[self.pos];
            self.pos += 1;
            return Some((r, 1));
        }
        None
    }
}

struct Utf16RuneReader<'a> {
    s: &'a [u16],
    pos: usize,
}

impl regexp::RuneReader for Utf16RuneReader<'_> {
    fn read_rune(&mut self) -> Option<(i32, usize)> {
        if self.pos < self.s.len() {
            let r = i32::from(self.s[self.pos]);
            self.pos += 1;
            return Some((r, 1));
        }
        None
    }
}

#[derive(Clone, Debug)]
pub struct RegexpPattern {
    pub src: Vec<u8>,
    pub global: bool,
    pub ignore_case: bool,
    pub multiline: bool,
    pub dot_all: bool,
    pub sticky: bool,
    pub unicode: bool,
    pub regexp_wrapper: Option<Arc<RegexpWrapper>>,
    pub regexp2_wrapper: Option<Regexp2Wrapper>,
}

pub fn compile_regexp2(
    src: &[u8],
    multiline: bool,
    dot_all: bool,
    ignore_case: bool,
    unicode: bool,
) -> Result<Regexp2Wrapper, regexp2::Error> {
    let mut opts: regexp2::RegexOptions = regexp2::ECMASCRIPT;
    if multiline {
        opts |= regexp2::MULTILINE;
    }
    if dot_all {
        opts |= regexp2::SINGLELINE;
    }
    if ignore_case {
        opts |= regexp2::IGNORE_CASE;
    }
    if unicode {
        opts |= regexp2::UNICODE;
    }
    let rx = regexp2::compile(src, opts)?;
    Ok(Regexp2Wrapper {
        rx: Arc::new(rx),
        cache: None,
    })
}

impl RegexpPattern {
    pub fn create_regexp2(&mut self) {
        if self.regexp2_wrapper.is_some() {
            return;
        }
        match compile_regexp2(
            &self.src,
            self.multiline,
            self.dot_all,
            self.ignore_case,
            self.unicode,
        ) {
            Ok(rx) => self.regexp2_wrapper = Some(rx),
            Err(e) => panic!(
                "Invalid regular expression: /{}/: {}",
                String::from_utf8_lossy(&self.src),
                e.reason()
            ),
        }
    }

    fn regexp2(&mut self) -> &mut Regexp2Wrapper {
        match &mut self.regexp2_wrapper {
            Some(w) => w,
            None => unreachable!("a regexp pattern without an engine"),
        }
    }

    pub fn find_submatch_index(&mut self, s: &JsString, start: isize) -> Option<Vec<isize>> {
        let unicode = self.unicode;
        let do_cache = self.global || self.sticky;
        let Some(w) = self.regexp_wrapper.clone() else {
            return self
                .regexp2()
                .find_submatch_index(s, start, unicode, do_cache);
        };
        if start != 0 {
            self.create_regexp2();
            return self
                .regexp2()
                .find_submatch_index(s, start, unicode, do_cache);
        }
        w.find_submatch_index(s, unicode)
    }

    pub fn find_all_submatch_index(
        &mut self,
        s: &JsString,
        start: isize,
        limit: isize,
        sticky: bool,
    ) -> Option<Vec<Vec<isize>>> {
        let unicode = self.unicode;
        let Some(w) = self.regexp_wrapper.clone() else {
            return self
                .regexp2()
                .find_all_submatch_index(s, start, limit, sticky, unicode);
        };
        if start == 0 {
            let (a, u) = devirtualize_string(s);
            let Some(u) = u else {
                return match &a {
                    Some(a) => w.find_all_submatch_index(a.as_bytes(), limit, sticky),
                    None => w.find_all_submatch_index(b"", limit, sticky),
                };
            };
            if limit == 1 {
                let result = w.find_submatch_index_unicode(&u, unicode)?;
                return Some(vec![result]);
            }
            if unicode && let Some((pm, s)) = build_utf8_pos_map(&u) {
                let mut res = w.find_all_submatch_index(&s, limit, sticky);
                if let Some(res) = &mut res {
                    for result in res.iter_mut() {
                        for idx in result.iter_mut() {
                            *idx = pm.get(*idx);
                        }
                    }
                }
                return res;
            }
        }
        self.create_regexp2();
        self.regexp2()
            .find_all_submatch_index(s, start, limit, sticky, unicode)
    }
}

fn build_utf8_pos_map(s: &UnicodeString) -> Option<(PositionMap, Vec<u8>)> {
    let units = &s.raw()[1..];
    let mut pm = Vec::with_capacity(s.length());
    let mut sb = Vec::with_capacity(units.len());
    let (mut s_pos, mut utf8_pos) = (0isize, 0isize);
    let mut i = 0;
    while i < units.len() {
        let c = units[i];
        let mut size = 1;
        let r = if crate::string::is_utf16_first_surrogate(c) {
            if i + 1 < units.len() && crate::string::is_utf16_second_surrogate(units[i + 1]) {
                size = 2;
                utf16::decode_rune(i32::from(c), i32::from(units[i + 1]))
            } else {
                return None;
            }
        } else if crate::string::is_utf16_second_surrogate(c) {
            return None;
        } else {
            i32::from(c)
        };
        let before = sb.len();
        utf8::append_rune(&mut sb, r);
        i += size;
        s_pos += size as isize;
        utf8_pos += (sb.len() - before) as isize;
        pm.push(PositionMapItem {
            src: utf8_pos,
            dst: s_pos,
        });
    }
    Some((PositionMap(pm), sb))
}

fn build_pos_map(s: &StrUnits, l: usize, start: isize) -> (Vec<isize>, Vec<i32>, isize, bool) {
    let mut pos_map = Vec::with_capacity(l + 1);
    let mut cur_pos = 0isize;
    let mut runes = Vec::with_capacity(l);
    let mut start_found = false;
    let mut mapped_start = 0isize;
    let mut split_pair = false;
    let len = s.len();
    let mut i = 0usize;
    loop {
        if !start_found {
            if cur_pos == start {
                mapped_start = runes.len() as isize;
                start_found = true;
            }
            if cur_pos > start {
                mapped_start = runes.len() as isize - 1;
                split_pair = true;
                start_found = true;
            }
        }
        if i >= len {
            break;
        }
        let (rn, size) = lenient_decode_at(s, i);
        i += size;
        runes.push(rn);
        pos_map.push(cur_pos);
        cur_pos += size as isize;
    }
    pos_map.push(cur_pos);
    (pos_map, runes, mapped_start, split_pair)
}

fn pos_map_reverse_lookup(pos_map: &[isize], pos: isize) -> (isize, bool) {
    let mapped = pos_map.partition_point(|&p| p < pos);
    if mapped < pos_map.len() && pos_map[mapped] != pos {
        return (mapped as isize - 1, true);
    }
    (mapped as isize, false)
}

fn utf16_runes(s: &StrUnits) -> Arc<[i32]> {
    match s {
        StrUnits::Ascii(a) => a.as_bytes().iter().map(|&c| i32::from(c)).collect(),
        StrUnits::Unicode(u) => u.raw()[1..].iter().map(|&c| i32::from(c)).collect(),
    }
}

fn same_target(cache: &Regexp2MatchCache, s: &JsString) -> bool {
    cache.target.same_as(&Value::String(s.clone()))
}

fn groups_to_result(m: &regexp2::Match<'_>, map: Option<&[isize]>) -> Vec<isize> {
    let groups = m.groups();
    let mut result = Vec::with_capacity(groups.len() << 1);
    for group in groups {
        if !group.captures.is_empty() {
            let (start, end) = (group.index, group.index + group.length);
            match map {
                Some(pm) => {
                    result.push(pm[start as usize]);
                    result.push(pm[end as usize]);
                }
                None => {
                    result.push(start);
                    result.push(end);
                }
            }
        } else {
            result.push(-1);
            result.push(0);
        }
    }
    result
}

impl Regexp2Wrapper {
    pub fn find_submatch_index(
        &mut self,
        s: &JsString,
        start: isize,
        full_unicode: bool,
        do_cache: bool,
    ) -> Option<Vec<isize>> {
        if full_unicode {
            return self.find_submatch_index_unicode(s, start, do_cache);
        }
        self.find_submatch_index_utf16(s, start, do_cache)
    }

    fn find_utf16_cached<'r>(
        cache: &mut Option<Regexp2MatchCache>,
        rx: &'r regexp2::Regexp,
        s: &JsString,
        start: isize,
        do_cache: bool,
    ) -> (
        Result<Option<regexp2::Match<'r>>, regexp2::Error>,
        Arc<[i32]>,
    ) {
        let cached = match cache {
            Some(c) if c.pos_map.is_none() && same_target(c, s) => Some(c.runes.clone()),
            _ => None,
        };
        let from_cache = cached.is_some();
        let runes = match cached {
            Some(r) => r,
            None => utf16_runes(&StrUnits::of(s)),
        };
        let m = rx.find_runes_match_starting_at(runes.clone(), start);
        if do_cache && matches!(m, Ok(Some(_))) {
            if !from_cache {
                *cache = Some(Regexp2MatchCache {
                    target: s.clone(),
                    runes: runes.clone(),
                    pos_map: None,
                });
            }
        } else {
            *cache = None;
        }
        (m, runes)
    }

    pub fn find_submatch_index_utf16(
        &mut self,
        s: &JsString,
        start: isize,
        do_cache: bool,
    ) -> Option<Vec<isize>> {
        let rx = self.rx.clone();
        let (m, _) = Regexp2Wrapper::find_utf16_cached(&mut self.cache, &rx, s, start, do_cache);
        match m {
            Ok(Some(m)) => Some(groups_to_result(&m, None)),
            _ => None,
        }
    }

    fn find_unicode_cached<'r>(
        cache: &mut Option<Regexp2MatchCache>,
        rx: &'r regexp2::Regexp,
        s: &JsString,
        start: isize,
        do_cache: bool,
    ) -> (
        Result<Option<regexp2::Match<'r>>, regexp2::Error>,
        Arc<[isize]>,
    ) {
        let cached = match cache {
            Some(c) => match &c.pos_map {
                Some(pm) if same_target(c, s) => Some((c.runes.clone(), pm.clone())),
                _ => None,
            },
            None => None,
        };
        let from_cache = cached.is_some();
        let (runes, pos_map, mapped_start, split_pair) = match cached {
            Some((runes, pos_map)) => {
                let (mapped_start, split_pair) = pos_map_reverse_lookup(&pos_map, start);
                (runes, pos_map, mapped_start, split_pair)
            }
            None => {
                let units = StrUnits::of(s);
                let (pos_map, runes, mapped_start, split_pair) =
                    build_pos_map(&units, units.len(), start);
                (
                    Arc::from(runes),
                    Arc::from(pos_map),
                    mapped_start,
                    split_pair,
                )
            }
        };
        let input: Arc<[i32]> = if split_pair {
            let mut v = runes.to_vec();
            let (_, second) = utf16::encode_rune(v[mapped_start as usize]);
            v[mapped_start as usize] = second;
            Arc::from(v)
        } else {
            runes.clone()
        };
        let m = rx.find_runes_match_starting_at(input, mapped_start);
        if do_cache && matches!(m, Ok(Some(_))) {
            if !from_cache {
                *cache = Some(Regexp2MatchCache {
                    target: s.clone(),
                    runes,
                    pos_map: Some(pos_map.clone()),
                });
            }
        } else {
            *cache = None;
        }
        (m, pos_map)
    }

    pub fn find_submatch_index_unicode(
        &mut self,
        s: &JsString,
        start: isize,
        do_cache: bool,
    ) -> Option<Vec<isize>> {
        let rx = self.rx.clone();
        let (m, pos_map) =
            Regexp2Wrapper::find_unicode_cached(&mut self.cache, &rx, s, start, do_cache);
        match m {
            Ok(Some(m)) => Some(groups_to_result(&m, Some(&pos_map))),
            _ => None,
        }
    }

    pub fn find_all_submatch_index_utf16(
        &mut self,
        s: &JsString,
        mut start: isize,
        mut limit: isize,
        sticky: bool,
    ) -> Option<Vec<Vec<isize>>> {
        let rx = self.rx.clone();
        let (m, runes) = Regexp2Wrapper::find_utf16_cached(&mut self.cache, &rx, s, start, false);
        let mut m = match m {
            Ok(Some(m)) => m,
            _ => return None,
        };
        if limit < 0 {
            limit = runes.len() as isize + 1;
        }
        let mut results = Vec::with_capacity(limit.min(runes.len() as isize + 1) as usize);
        loop {
            let result = groups_to_result(&m, None);
            if sticky && result.len() > 1 {
                if result[0] != start {
                    break;
                }
                start = result[1];
            }
            results.push(result);
            limit -= 1;
            if limit <= 0 {
                break;
            }
            match rx.find_next_match(&m) {
                Ok(Some(next)) => m = next,
                Ok(None) => break,
                Err(_) => return None,
            }
        }
        Some(results)
    }

    pub fn find_all_submatch_index_unicode(
        &mut self,
        s: &JsString,
        mut start: isize,
        mut limit: isize,
        sticky: bool,
    ) -> Option<Vec<Vec<isize>>> {
        let rx = self.rx.clone();
        if limit < 0 {
            limit = s.length() as isize + 2;
        }
        let mut results = Vec::with_capacity(limit.min(s.length() as isize + 2) as usize);
        let (m, pos_map) =
            Regexp2Wrapper::find_unicode_cached(&mut self.cache, &rx, s, start, false);
        let mut m = match m {
            Ok(m) => m,
            Err(_) => return None,
        };
        while let Some(cur) = m {
            let result = groups_to_result(&cur, Some(&pos_map));
            if sticky && result.len() > 1 {
                if result[0] != start {
                    break;
                }
                start = result[1];
            }
            results.push(result);
            m = match rx.find_next_match(&cur) {
                Ok(next) => next,
                Err(_) => return None,
            };
        }
        Some(results)
    }

    pub fn find_all_submatch_index(
        &mut self,
        s: &JsString,
        start: isize,
        limit: isize,
        sticky: bool,
        full_unicode: bool,
    ) -> Option<Vec<Vec<isize>>> {
        if !s.is_ascii() {
            if full_unicode {
                return self.find_all_submatch_index_unicode(s, start, limit, sticky);
            }
            return self.find_all_submatch_index_utf16(s, start, limit, sticky);
        }
        self.find_all_submatch_index_utf16(s, start, limit, sticky)
    }
}

impl RegexpWrapper {
    pub fn find_all_submatch_index(
        &self,
        s: &[u8],
        limit: isize,
        sticky: bool,
    ) -> Option<Vec<Vec<isize>>> {
        let mut results = self.0.find_all_string_submatch_index(s, limit);
        let mut pos = 0;
        if sticky && let Some(res) = &mut results {
            for i in 0..res.len() {
                let result = &res[i];
                if result.len() > 1 {
                    if result[0] != pos {
                        res.truncate(i);
                        return results;
                    }
                    pos = result[1];
                }
            }
        }
        results
    }

    pub fn find_submatch_index(&self, s: &JsString, full_unicode: bool) -> Option<Vec<isize>> {
        let (a, u) = devirtualize_string(s);
        if let Some(u) = u {
            return self.find_submatch_index_unicode(&u, full_unicode);
        }
        self.find_submatch_index_ascii(a.as_ref().map(|a| a.as_bytes()).unwrap_or_default())
    }

    pub fn find_submatch_index_ascii(&self, s: &[u8]) -> Option<Vec<isize>> {
        self.0.find_string_submatch_index(s)
    }

    pub fn find_submatch_index_unicode(
        &self,
        s: &UnicodeString,
        full_unicode: bool,
    ) -> Option<Vec<isize>> {
        if full_unicode {
            let units = StrUnits::Unicode(s.clone());
            let (pos_map, runes, _, _) = build_pos_map(&units, s.length(), 0);
            let mut res = self.0.find_reader_submatch_index(&mut ArrayRuneReader {
                runes: &runes,
                pos: 0,
            });
            if let Some(res) = &mut res {
                for item in res.iter_mut() {
                    if *item >= 0 {
                        *item = pos_map[*item as usize];
                    }
                }
            }
            return res;
        }
        self.0.find_reader_submatch_index(&mut Utf16RuneReader {
            s: &s.raw()[1..],
            pos: 0,
        })
    }
}

pub struct RegexpObject {
    pub pattern: Rc<RefCell<RegexpPattern>>,
    pub source: JsString,
    pub standard: bool,
}

impl ObjData for RegexpObject {
    fn trace(&self, _t: &mut Tracer) {}

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub static REGEXP_OBJECT_CLASS: ObjClass = ObjClass {
    set_proto: RegexpObject::set_proto,
    define_own_property_str: RegexpObject::define_own_property_str,
    define_own_property_sym: RegexpObject::define_own_property_sym,
    delete_str: RegexpObject::delete_str,
    set_own_str: RegexpObject::set_own_str,
    set_own_sym: RegexpObject::set_own_sym,
    ..BASE_CLASS
};

fn is_std_symbol(s: SymRef) -> bool {
    s == SYM_MATCH || s == SYM_MATCH_ALL || s == SYM_SEARCH || s == SYM_SPLIT || s == SYM_REPLACE
}

impl RegexpObject {
    pub fn data(rt: &Runtime, o: ObjRef) -> Option<&RegexpObject> {
        rt.ext_dyn::<RegexpObject>(o)
    }

    pub fn pattern_of(rt: &Runtime, o: ObjRef) -> Rc<RefCell<RegexpPattern>> {
        match RegexpObject::data(rt, o) {
            Some(r) => r.pattern.clone(),
            None => unreachable!("not a regexp object"),
        }
    }

    pub fn source_of(rt: &Runtime, o: ObjRef) -> JsString {
        match RegexpObject::data(rt, o) {
            Some(r) => r.source.clone(),
            None => unreachable!("not a regexp object"),
        }
    }

    fn is_standard(rt: &Runtime, o: ObjRef) -> bool {
        RegexpObject::data(rt, o).is_some_and(|r| r.standard)
    }

    fn set_standard(rt: &mut Runtime, o: ObjRef, standard: bool) {
        if let Some(r) = rt.ext_dyn_mut::<RegexpObject>(o) {
            r.standard = standard;
        }
    }

    pub fn exec_result_to_array(
        rt: &mut Runtime,
        target: &JsString,
        result: &[isize],
    ) -> JsResult<Value> {
        let capture_count = result.len() >> 1;
        let mut value_array = Vec::with_capacity(capture_count);
        let match_index = result[0];
        value_array.push(Value::String(
            target.substring(result[0] as usize, result[1] as usize),
        ));
        let mut lower_bound = 0;
        for index in 1..capture_count {
            let offset = index << 1;
            if result[offset] >= 0 && result[offset + 1] >= lower_bound {
                value_array.push(Value::String(
                    target.substring(result[offset] as usize, result[offset + 1] as usize),
                ));
                lower_bound = result[offset];
            } else {
                value_array.push(Value::Undefined);
            }
        }
        let m = rt.new_array_values(value_array);
        m.set_own_str(rt, &INPUT, Value::String(target.clone()), false)?;
        m.set_own_str(rt, &INDEX, int_to_value(match_index as i64), false)?;
        Ok(Value::Object(m))
    }

    pub fn get_last_index(rt: &mut Runtime, o: ObjRef) -> JsResult<i64> {
        let v = nil_safe(o.get_str(rt, &LAST_INDEX, None)?);
        let last_index = to_length(rt, &v)?;
        let pattern = RegexpObject::pattern_of(rt, o);
        let p = pattern.borrow();
        if !p.global && !p.sticky {
            return Ok(0);
        }
        Ok(last_index)
    }

    pub fn exec_regexp(
        rt: &mut Runtime,
        o: ObjRef,
        target: &JsString,
    ) -> JsResult<(bool, Option<Vec<isize>>)> {
        let index = RegexpObject::get_last_index(rt, o)?;
        let pattern = RegexpObject::pattern_of(rt, o);
        let mut result = None;
        if index >= 0 && index <= target.length() as i64 {
            result = pattern
                .borrow_mut()
                .find_submatch_index(target, index as isize);
        }
        let (global, sticky) = {
            let p = pattern.borrow();
            (p.global, p.sticky)
        };
        let matched = match &result {
            Some(r) => !r.is_empty() && (!sticky || r[0] as i64 == index),
            None => false,
        };
        if global || sticky {
            let mut new_last_index = 0;
            if matched && let Some(r) = &result {
                new_last_index = r[1] as i64;
            }
            o.set_own_str(rt, &LAST_INDEX, int_to_value(new_last_index), true)?;
        }
        Ok((matched, result))
    }

    pub fn exec(rt: &mut Runtime, o: ObjRef, target: &JsString) -> JsResult<Value> {
        let (matched, result) = RegexpObject::exec_regexp(rt, o, target)?;
        if matched && let Some(result) = result {
            return RegexpObject::exec_result_to_array(rt, target, &result);
        }
        Ok(Value::Null)
    }

    pub fn test(rt: &mut Runtime, o: ObjRef, target: &JsString) -> JsResult<bool> {
        let (matched, _) = RegexpObject::exec_regexp(rt, o, target)?;
        Ok(matched)
    }

    pub fn clone(rt: &mut Runtime, o: ObjRef) -> ObjRef {
        let proto = rt.obj(o).base.prototype;
        let pattern = RegexpObject::pattern_of(rt, o);
        let source = RegexpObject::source_of(rt, o);
        rt.new_regexp_object(proto, pattern, source)
    }

    pub fn init(rt: &mut Runtime, o: ObjRef) {
        RegexpObject::set_standard(rt, o, true);
        o._put_prop(rt, &LAST_INDEX, int_to_value(0), true, false, false);
    }

    pub fn set_proto(
        rt: &mut Runtime,
        o: ObjRef,
        proto: Option<ObjRef>,
        throw: bool,
    ) -> JsResult<bool> {
        let res = BaseObject::set_proto(rt, o, proto, throw)?;
        if res {
            RegexpObject::set_standard(rt, o, false);
        }
        Ok(res)
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let res = BaseObject::define_own_property_str(rt, o, name, desc, throw)?;
        if res {
            RegexpObject::set_standard(rt, o, false);
        }
        Ok(res)
    }

    pub fn define_own_property_sym(
        rt: &mut Runtime,
        o: ObjRef,
        name: SymRef,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let res = BaseObject::define_own_property_sym(rt, o, name, desc, throw)?;
        if res && RegexpObject::is_standard(rt, o) && is_std_symbol(name) {
            RegexpObject::set_standard(rt, o, false);
        }
        Ok(res)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        let res = BaseObject::delete_str(rt, o, name, throw)?;
        if res {
            RegexpObject::set_standard(rt, o, false);
        }
        Ok(res)
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        value: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let res = BaseObject::set_own_str(rt, o, name, value, throw)?;
        if res && RegexpObject::is_standard(rt, o) && *name == *EXEC {
            RegexpObject::set_standard(rt, o, false);
        }
        Ok(res)
    }

    pub fn set_own_sym(
        rt: &mut Runtime,
        o: ObjRef,
        name: SymRef,
        value: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let res = BaseObject::set_own_sym(rt, o, name, value, throw)?;
        if res && RegexpObject::is_standard(rt, o) && is_std_symbol(name) {
            RegexpObject::set_standard(rt, o, false);
        }
        Ok(res)
    }
}

impl Runtime {
    pub fn new_regexp_object(
        &mut self,
        proto: Option<ObjRef>,
        pattern: Rc<RefCell<RegexpPattern>>,
        source: JsString,
    ) -> ObjRef {
        let o = self.alloc_object(
            &REGEXP_OBJECT_CLASS,
            BaseObject::new(crate::object::CLASS_REG_EXP, proto),
            crate::object::ObjExt::Dyn(Box::new(RegexpObject {
                pattern,
                source,
                standard: false,
            })),
        );
        RegexpObject::init(self, o);
        o
    }
}
