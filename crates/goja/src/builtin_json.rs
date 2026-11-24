use crate::builtin_symbol::SYM_TO_STRING_TAG;
use crate::func::NativeFn;
use crate::heap::ObjRef;
use crate::object::{
    CLASS_JSON, CLASS_NUMBER, CLASS_OBJECT, CLASS_STRING, CallFn, FunctionCall, ObjExt,
};
use crate::proxy::proxy_class_name;
use crate::runtime::{
    JsResult, Runtime, Throw, create_data_property, create_data_property_or_throw, is_array,
    to_length,
};
use crate::string::{JsString, STRING_EMPTY, ascii_string, new_string_value};
use crate::string_ascii::AsciiString;
use crate::string_imported::ImportedString;
use crate::unistring;
use crate::value::{Value, float_to_value, go_float_to_int, int_to_ascii, nil_safe, value_prop};
use gocompat::json::{Decoder, Error as JsonError, Subject, Token};
use std::collections::HashSet;
use std::io::Cursor;

const HEX: &[u8; 16] = b"0123456789abcdef";

enum ParseFrame {
    Object {
        object: ObjRef,
        key: Option<unistring::String>,
    },
    Array(Vec<Value>),
}

fn json_syntax_error(rt: &mut Runtime, msg: &str) -> Throw {
    let e = rt.get_syntax_error();
    Throw::Value(rt.new_error(e, msg))
}

fn decode_error(rt: &mut Runtime, err: &JsonError) -> Throw {
    let msg = err.message(Subject::Value);
    json_syntax_error(rt, &msg)
}

fn display_char(rest: &[u8]) -> String {
    let c = rest[0];
    if c < 0x20 {
        return format!("\\^{}", (c ^ 0x40) as char);
    }
    if c == 0x7f {
        return "\\^?".to_string();
    }
    if c < 0x80 {
        return (c as char).to_string();
    }
    let (_, size) = gocompat::utf8::decode_rune(rest);
    String::from_utf8_lossy(&rest[..size]).into_owned()
}

fn trailing_error(rt: &mut Runtime, input: &[u8], offset: usize) -> Throw {
    let mut j = offset.min(input.len());
    while j < input.len() && matches!(input[j], b' ' | b'\t' | b'\n' | b'\r') {
        j += 1;
    }
    if j >= input.len() {
        return json_syntax_error(rt, "invalid JSON: unexpected end of input");
    }
    let msg = format!(
        "invalid JSON at byte {}: unexpected character '{}'",
        j + 1,
        display_char(&input[j..])
    );
    json_syntax_error(rt, &msg)
}

fn unexpected_token(rt: &mut Runtime, tok: &Token) -> Throw {
    let msg = match tok {
        Token::Delim(c) => format!("Unexpected token: {c}"),
        Token::Null => "Unexpected token: null".to_string(),
        Token::Bool(b) => format!("Unexpected token: {b}"),
        Token::Number(n) => format!("Unexpected token: {n}"),
        Token::String(s) => format!("Unexpected token: {s}"),
    };
    json_syntax_error(rt, &msg)
}

enum Revive {
    Leaf,
    Array {
        object: ObjRef,
        length: i64,
        index: i64,
    },
    Object {
        object: ObjRef,
        keys: Vec<Value>,
        index: usize,
    },
}

struct ReviveFrame {
    holder: ObjRef,
    name: Value,
    value: Value,
    walk: Revive,
}

impl Runtime {
    pub fn builtin_json_parse(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let input = call.arg(0).to_string(self)?.go_string();
        let mut d = Decoder::new(Cursor::new(&input[..]));

        let value = match self.builtin_json_decode_value(&mut d) {
            Ok(v) => v,
            Err(JsonResult::Js(t)) => return Err(t),
            Err(JsonResult::Json(e)) => return Err(decode_error(self, &e)),
        };

        let offset = d.input_offset() as usize;
        match d.token() {
            Err(e) if e.is_eof() => {}
            _ => return Err(trailing_error(self, &input, offset)),
        }

        let mut reviver = None;
        let arg1 = call.arg(1);
        if !arg1.is_undefined() {
            let o = arg1.to_object(self)?;
            reviver = o.assert_callable(self);
        }

        if let Some(reviver) = reviver {
            let root = self.new_object();
            create_data_property_or_throw(self, root, &Value::String(STRING_EMPTY.clone()), value)?;
            return self.builtin_json_revive_walk(
                reviver,
                root,
                Value::String(STRING_EMPTY.clone()),
            );
        }

        Ok(value)
    }

    fn builtin_json_decode_value(
        &mut self,
        d: &mut Decoder<Cursor<&[u8]>>,
    ) -> Result<Value, JsonResult> {
        let mut stack: Vec<ParseFrame> = Vec::new();
        let mut tok = d.token().map_err(JsonResult::Json)?;
        loop {
            let mut completed = match tok {
                Token::Delim('{') => {
                    let object = self.new_object();
                    stack.push(ParseFrame::Object { object, key: None });
                    None
                }
                Token::Delim('[') => {
                    stack.push(ParseFrame::Array(Vec::new()));
                    None
                }
                Token::Null => Some(Value::Null),
                Token::String(s) => Some(Value::String(new_string_value(s.as_bytes()))),
                Token::Number(f) => Some(float_to_value(f)),
                Token::Bool(b) => Some(Value::Bool(b)),
                other => return Err(JsonResult::Js(unexpected_token(self, &other))),
            };
            loop {
                if let Some(v) = completed.take() {
                    match stack.last_mut() {
                        None => return Ok(v),
                        Some(ParseFrame::Object { object, key }) => {
                            if let Some(k) = key.take() {
                                object._put_prop(self, &k, v, true, true, true);
                            }
                        }
                        Some(ParseFrame::Array(values)) => values.push(v),
                    }
                }
                match stack.last_mut() {
                    None => unreachable!("empty decode stack"),
                    Some(ParseFrame::Object { object, key }) => {
                        let object = *object;
                        match d.token().map_err(JsonResult::Json)? {
                            Token::Delim('}') => {
                                stack.pop();
                                completed = Some(Value::Object(object));
                            }
                            Token::String(k) => {
                                *key = Some(unistring::String::new(&k));
                                tok = d.token().map_err(JsonResult::Json)?;
                                break;
                            }
                            other => return Err(JsonResult::Js(unexpected_token(self, &other))),
                        }
                    }
                    Some(ParseFrame::Array(_)) => {
                        let t = d.token().map_err(JsonResult::Json)?;
                        if let Token::Delim(']') = t {
                            let Some(ParseFrame::Array(values)) = stack.pop() else {
                                unreachable!("array frame");
                            };
                            completed = Some(Value::Object(self.new_array_values(values)));
                        } else {
                            tok = t;
                            break;
                        }
                    }
                }
            }
        }
    }

    fn builtin_json_revive_frame(&mut self, holder: ObjRef, name: Value) -> JsResult<ReviveFrame> {
        let value = nil_safe(holder.get(self, &name, None)?);
        let mut walk = Revive::Leaf;
        if let Value::Object(object) = &value {
            let object = *object;
            if is_array(self, object)? {
                let lv = nil_safe(object.get_str(self, &"length".into(), None)?);
                let length = to_length(self, &lv)?;
                walk = Revive::Array {
                    object,
                    length,
                    index: 0,
                };
            } else {
                let keys = object.string_keys(self, false, Vec::new())?;
                walk = Revive::Object {
                    object,
                    keys,
                    index: 0,
                };
            }
        }
        Ok(ReviveFrame {
            holder,
            name,
            value,
            walk,
        })
    }

    pub fn builtin_json_revive_walk(
        &mut self,
        reviver: CallFn,
        holder: ObjRef,
        name: Value,
    ) -> JsResult<Value> {
        let first = self.builtin_json_revive_frame(holder, name)?;
        let mut stack = vec![first];
        loop {
            let Some(top) = stack.last_mut() else {
                unreachable!("empty revive stack");
            };
            let child = match &mut top.walk {
                Revive::Leaf => None,
                Revive::Array {
                    object,
                    length,
                    index,
                } => {
                    if *index < *length {
                        let n = Value::String(JsString::Ascii(int_to_ascii(*index)));
                        *index += 1;
                        Some((*object, n))
                    } else {
                        None
                    }
                }
                Revive::Object {
                    object,
                    keys,
                    index,
                } => {
                    if *index < keys.len() {
                        let n = keys[*index].clone();
                        *index += 1;
                        Some((*object, n))
                    } else {
                        None
                    }
                }
            };
            if let Some((object, n)) = child {
                let frame = self.builtin_json_revive_frame(object, n)?;
                stack.push(frame);
                continue;
            }
            let Some(done) = stack.pop() else {
                unreachable!("empty revive stack");
            };
            let ReviveFrame {
                holder,
                name,
                value,
                ..
            } = done;
            let result = reviver.call(self, Value::Object(holder), &[name.clone(), value])?;
            let Some(parent) = stack.last() else {
                return Ok(result);
            };
            match &parent.walk {
                Revive::Array { object, .. } => {
                    let object = *object;
                    if result.is_undefined() {
                        object.delete(self, &name, false)?;
                    } else {
                        create_data_property(self, object, &name, result)?;
                    }
                }
                Revive::Object { object, .. } => {
                    let object = *object;
                    if result.is_undefined() {
                        let s = name.string(self)?;
                        object.delete_str(self, &s, false)?;
                    } else {
                        create_data_property(self, object, &name, result)?;
                    }
                }
                Revive::Leaf => unreachable!("leaf parent"),
            }
        }
    }

    pub fn builtin_json_stringify(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut ctx = BuiltinJsonStringifyContext {
            stack: Vec::new(),
            property_list: None,
            replacer_function: None,
            gap: Vec::new(),
            indent: Vec::new(),
            buf: Vec::new(),
            all_ascii: true,
        };

        if let Value::Object(replacer) = call.arg(1) {
            let replacer = *replacer;
            if is_array(self, replacer)? {
                let lv = nil_safe(replacer.get_str(self, &"length".into(), None)?);
                let length = to_length(self, &lv)?;
                let mut seen: HashSet<String> = HashSet::new();
                let mut property_list = Vec::new();
                for index in 0..length {
                    let value = nil_safe(replacer.get_idx(self, index, None)?);
                    let name = match &value {
                        Value::Float(_) | Value::Int(_) | Value::String(_) => {
                            value.go_string(self)?
                        }
                        Value::Object(v) => match proxy_class_name(self, *v)? {
                            CLASS_NUMBER | CLASS_STRING => value.go_string(self)?,
                            _ => continue,
                        },
                        _ => continue,
                    };
                    if seen.contains(&name) {
                        continue;
                    }
                    property_list.push(Value::String(new_string_value(name.as_bytes())));
                    seen.insert(name);
                }
                ctx.property_list = Some(property_list);
            } else if let Some(c) = replacer.assert_callable(self) {
                ctx.replacer_function = Some(c);
            }
        }
        let mut space_value = call.argument(2);
        if !space_value.is_undefined() {
            if let Value::Object(o) = &space_value {
                let o = *o;
                match &self.obj(o).ext {
                    ObjExt::Primitive(Value::Int(_) | Value::Float(_)) => {
                        space_value = Value::Object(o).to_number(self)?;
                    }
                    ObjExt::StringObj(_) => {
                        space_value = Value::String(Value::Object(o).to_string(self)?);
                    }
                    _ => {}
                }
            }
            let mut is_num = false;
            let mut num: i64 = 0;
            match &space_value {
                Value::Int(i) => {
                    num = *i;
                    is_num = true;
                }
                Value::Float(f) => {
                    num = go_float_to_int(*f);
                    is_num = true;
                }
                _ => {}
            }
            if is_num {
                if num > 0 {
                    if num > 10 {
                        num = 10;
                    }
                    ctx.gap = vec![b' '; num as usize];
                }
            } else if let Value::String(s) = &space_value {
                let str_ = s.go_string();
                if str_.len() > 10 {
                    ctx.gap = str_[..10].to_vec();
                } else {
                    ctx.gap = str_;
                }
            }
        }

        if ctx.r#do(self, call.argument(0))? {
            if ctx.all_ascii {
                return Ok(Value::String(JsString::Ascii(AsciiString::new(&ctx.buf))));
            }
            return Ok(Value::String(JsString::Imported(ImportedString::new(
                &ctx.buf,
            ))));
        }
        Ok(Value::Undefined)
    }

    pub fn get_json(&mut self) -> ObjRef {
        if let Some(r) = self.global.json {
            return r;
        }
        let op = self.global.object_prototype;
        let json = self.new_base_object(op, CLASS_OBJECT);
        self.global.json = Some(json);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.builtin_json_parse(c)), "parse", 2);
        json._put_prop(self, &"parse".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.builtin_json_stringify(c)),
            "stringify",
            3,
        );
        json._put_prop(
            self,
            &"stringify".into(),
            Value::Object(f),
            true,
            false,
            true,
        );
        json._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(Value::String(ascii_string(CLASS_JSON)), false, false, true),
        );
        json
    }
}

enum JsonResult {
    Js(Throw),
    Json(JsonError),
}

enum Str {
    Done(bool),
    Enter(StringifyFrame),
}

enum StringifyFrame {
    Array {
        array: ObjRef,
        stepback: Vec<u8>,
        length: i64,
        i: i64,
        separator: Vec<u8>,
        awaiting: bool,
    },
    Object {
        object: ObjRef,
        stepback: Vec<u8>,
        mark: usize,
        separator: Vec<u8>,
        props: Vec<Value>,
        idx: usize,
        empty: bool,
        awaiting: bool,
    },
}

struct BuiltinJsonStringifyContext {
    stack: Vec<ObjRef>,
    property_list: Option<Vec<Value>>,
    replacer_function: Option<CallFn>,
    gap: Vec<u8>,
    indent: Vec<u8>,
    buf: Vec<u8>,
    all_ascii: bool,
}

impl BuiltinJsonStringifyContext {
    fn r#do(&mut self, rt: &mut Runtime, v: Value) -> JsResult<bool> {
        let holder = rt.new_object();
        create_data_property_or_throw(rt, holder, &Value::String(STRING_EMPTY.clone()), v)?;
        self.str(rt, Value::String(STRING_EMPTY.clone()), holder)
    }

    fn str(&mut self, rt: &mut Runtime, key: Value, holder: ObjRef) -> JsResult<bool> {
        let mut frames = match self.str_begin(rt, key, holder)? {
            Str::Done(b) => return Ok(b),
            Str::Enter(f) => vec![f],
        };
        loop {
            let Some(top) = frames.last_mut() else {
                return Ok(true);
            };
            let step = match top {
                StringifyFrame::Array {
                    array,
                    length,
                    i,
                    separator,
                    awaiting,
                    ..
                } => {
                    if *awaiting {
                        *awaiting = false;
                        if *i < *length - 1 {
                            self.buf.extend_from_slice(separator);
                        }
                        *i += 1;
                    }
                    if *i < *length {
                        let array = *array;
                        let k = Value::String(JsString::Ascii(int_to_ascii(*i)));
                        match self.str_begin(rt, k, array)? {
                            Str::Done(b) => {
                                let StringifyFrame::Array {
                                    length,
                                    i,
                                    separator,
                                    ..
                                } = frames.last_mut().unwrap_or_else(|| unreachable!())
                                else {
                                    unreachable!()
                                };
                                if !b {
                                    self.buf.extend_from_slice(b"null");
                                }
                                if *i < *length - 1 {
                                    self.buf.extend_from_slice(separator);
                                }
                                *i += 1;
                                None
                            }
                            Str::Enter(child) => {
                                if let Some(StringifyFrame::Array { awaiting, .. }) =
                                    frames.last_mut()
                                {
                                    *awaiting = true;
                                }
                                Some(child)
                            }
                        }
                    } else {
                        let Some(StringifyFrame::Array { stepback, .. }) = frames.pop() else {
                            unreachable!()
                        };
                        if !self.gap.is_empty() {
                            self.buf.push(b'\n');
                            self.buf.extend_from_slice(&stepback);
                            self.indent = stepback;
                        }
                        self.buf.push(b']');
                        self.stack.pop();
                        None
                    }
                }
                StringifyFrame::Object {
                    object,
                    props,
                    idx,
                    empty,
                    awaiting,
                    separator,
                    ..
                } => {
                    if *awaiting {
                        *awaiting = false;
                        if *empty {
                            *empty = false;
                        }
                        *idx += 1;
                    }
                    if *idx < props.len() {
                        let object = *object;
                        let name = props[*idx].clone();
                        let off = self.buf.len();
                        if !*empty {
                            self.buf.extend_from_slice(separator);
                        }
                        let n = name.to_string(rt)?;
                        self.quote(&n);
                        if !self.gap.is_empty() {
                            self.buf.extend_from_slice(b": ");
                        } else {
                            self.buf.push(b':');
                        }
                        match self.str_begin(rt, name, object)? {
                            Str::Done(b) => {
                                let Some(StringifyFrame::Object { idx, empty, .. }) =
                                    frames.last_mut()
                                else {
                                    unreachable!()
                                };
                                if b {
                                    if *empty {
                                        *empty = false;
                                    }
                                } else {
                                    self.buf.truncate(off);
                                }
                                *idx += 1;
                                None
                            }
                            Str::Enter(child) => {
                                if let Some(StringifyFrame::Object { awaiting, .. }) =
                                    frames.last_mut()
                                {
                                    *awaiting = true;
                                }
                                Some(child)
                            }
                        }
                    } else {
                        let Some(StringifyFrame::Object {
                            stepback,
                            mark,
                            empty,
                            ..
                        }) = frames.pop()
                        else {
                            unreachable!()
                        };
                        if empty {
                            self.buf.truncate(mark);
                        } else if !self.gap.is_empty() {
                            self.buf.push(b'\n');
                            self.buf.extend_from_slice(&stepback);
                            self.indent = stepback;
                        }
                        self.buf.push(b'}');
                        self.stack.pop();
                        None
                    }
                }
            };
            if let Some(child) = step {
                frames.push(child);
            }
        }
    }

    fn str_begin(&mut self, rt: &mut Runtime, key: Value, holder: ObjRef) -> JsResult<Str> {
        let mut value = nil_safe(holder.get(rt, &key, None)?);

        if let Value::Object(_) | Value::BigInt(_) = &value
            && let Some(Value::Object(to_json)) = rt.get_v_str(&value, &"toJSON".into())?
            && let Some(c) = to_json.assert_callable(rt)
        {
            value = c.call(rt, value, std::slice::from_ref(&key))?;
        }

        if let Some(rf) = self.replacer_function {
            value = rf.call(rt, Value::Object(holder), &[key, value])?;
        }

        if let Value::Object(o) = &value {
            let o = *o;
            match &rt.obj(o).ext {
                ObjExt::Primitive(p) => {
                    let p = p.clone();
                    match p {
                        Value::Int(_) | Value::Float(_) => {
                            value = Value::Object(o).to_number(rt)?;
                        }
                        p => value = p,
                    }
                }
                ObjExt::StringObj(_) => {
                    value = Value::String(Value::Object(o).to_string(rt)?);
                }
                _ => {}
            }
        }

        match &value {
            Value::Bool(b) => {
                if *b {
                    self.buf.extend_from_slice(b"true");
                } else {
                    self.buf.extend_from_slice(b"false");
                }
            }
            Value::String(s) => self.quote(s),
            Value::Int(_) => self
                .buf
                .extend_from_slice(&value.prim_to_string().go_string()),
            Value::Float(f) => {
                if !f.is_nan() && !f.is_infinite() {
                    self.buf
                        .extend_from_slice(&value.prim_to_string().go_string());
                } else {
                    self.buf.extend_from_slice(b"null");
                }
            }
            Value::Null => self.buf.extend_from_slice(b"null"),
            Value::BigInt(_) => {
                rt.type_error_result(true, "Do not know how to serialize a BigInt")?;
            }
            Value::Object(value1) => {
                let value1 = *value1;
                for i in 0..self.stack.len() {
                    let object = self.stack[i];
                    if value1 == object || value1.equal(rt, object) {
                        rt.type_error_result(true, "Converting circular structure to JSON")?;
                    }
                }
                self.stack.push(value1);
                if value1.assert_callable(rt).is_none() {
                    let r = if is_array(rt, value1)? {
                        self.ja(rt, value1)
                    } else {
                        self.jo(rt, value1)
                    };
                    return match r? {
                        Some(f) => Ok(Str::Enter(f)),
                        None => {
                            self.stack.pop();
                            Ok(Str::Done(true))
                        }
                    };
                }
                self.stack.pop();
                return Ok(Str::Done(false));
            }
            _ => return Ok(Str::Done(false)),
        }
        Ok(Str::Done(true))
    }

    fn ja(&mut self, rt: &mut Runtime, array: ObjRef) -> JsResult<Option<StringifyFrame>> {
        let mut stepback = Vec::new();
        if !self.gap.is_empty() {
            stepback = self.indent.clone();
            self.indent.extend_from_slice(&self.gap);
        }
        let lv = nil_safe(array.get_str(rt, &"length".into(), None)?);
        let length = to_length(rt, &lv)?;
        if length == 0 {
            self.buf.extend_from_slice(b"[]");
            return Ok(None);
        }

        self.buf.push(b'[');
        let separator = if !self.gap.is_empty() {
            self.buf.push(b'\n');
            self.buf.extend_from_slice(&self.indent);
            let mut s = b",\n".to_vec();
            s.extend_from_slice(&self.indent);
            s
        } else {
            b",".to_vec()
        };
        Ok(Some(StringifyFrame::Array {
            array,
            stepback,
            length,
            i: 0,
            separator,
            awaiting: false,
        }))
    }

    fn jo(&mut self, rt: &mut Runtime, object: ObjRef) -> JsResult<Option<StringifyFrame>> {
        let mut stepback = Vec::new();
        if !self.gap.is_empty() {
            stepback = self.indent.clone();
            self.indent.extend_from_slice(&self.gap);
        }

        self.buf.push(b'{');
        let mark = self.buf.len();
        let separator = if !self.gap.is_empty() {
            self.buf.push(b'\n');
            self.buf.extend_from_slice(&self.indent);
            let mut s = b",\n".to_vec();
            s.extend_from_slice(&self.indent);
            s
        } else {
            b",".to_vec()
        };

        let props = match &self.property_list {
            None => object.string_keys(rt, false, Vec::new())?,
            Some(p) => p.clone(),
        };

        Ok(Some(StringifyFrame::Object {
            object,
            stepback,
            mark,
            separator,
            props,
            idx: 0,
            empty: true,
            awaiting: false,
        }))
    }

    fn quote(&mut self, str_: &JsString) {
        self.buf.push(b'"');
        if let JsString::Ascii(a) = str_ {
            for &c in a.as_bytes() {
                self.quote_rune(i32::from(c));
            }
        } else {
            let mut reader = str_.utf16_reader();
            let mut prev: Option<u16> = None;
            loop {
                let c = match prev.take() {
                    Some(c) => c,
                    None => match reader.read_char() {
                        Some(c) => c,
                        None => break,
                    },
                };
                let mut r = i32::from(c);
                if (0xD800..=0xDBFF).contains(&c) {
                    match reader.read_char() {
                        None => {}
                        Some(second) => {
                            if (0xDC00..=0xDFFF).contains(&second) {
                                r = gocompat::utf16::decode_rune(r, i32::from(second));
                            } else {
                                prev = Some(second);
                            }
                        }
                    }
                }
                self.quote_rune(r);
            }
        }
        self.buf.push(b'"');
    }

    fn quote_rune(&mut self, r: i32) {
        match r {
            0x22 | 0x5C => {
                self.buf.push(b'\\');
                self.buf.push(r as u8);
            }
            0x08 => self.buf.extend_from_slice(b"\\b"),
            0x09 => self.buf.extend_from_slice(b"\\t"),
            0x0A => self.buf.extend_from_slice(b"\\n"),
            0x0C => self.buf.extend_from_slice(b"\\f"),
            0x0D => self.buf.extend_from_slice(b"\\r"),
            _ => {
                if r < 0x20 {
                    self.buf.extend_from_slice(b"\\u00");
                    self.buf.push(HEX[(r >> 4) as usize]);
                    self.buf.push(HEX[(r & 0xF) as usize]);
                } else if (0xD800..=0xDFFF).contains(&r) {
                    self.buf.extend_from_slice(b"\\u");
                    self.buf.push(HEX[(r >> 12) as usize]);
                    self.buf.push(HEX[((r >> 8) & 0xF) as usize]);
                    self.buf.push(HEX[((r >> 4) & 0xF) as usize]);
                    self.buf.push(HEX[(r & 0xF) as usize]);
                } else {
                    gocompat::utf8::append_rune(&mut self.buf, r);
                    if self.all_ascii && r >= 0x80 {
                        self.all_ascii = false;
                    }
                }
            }
        }
    }
}
