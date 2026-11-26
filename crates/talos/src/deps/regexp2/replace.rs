use super::r#match::Match;
use super::regexp::{Error, Regexp};
use super::syntax::{ReplacerData, push_rune, runes_to_string};

const REPLACE_SPECIALS: isize = 4;
const REPLACE_LEFT_PORTION: isize = -1;
const REPLACE_RIGHT_PORTION: isize = -2;
const REPLACE_LAST_GROUP: isize = -3;
const REPLACE_WHOLE_STRING: isize = -4;

pub(crate) type MatchEvaluator<'f> = &'f mut dyn FnMut(&Match<'_>) -> String;

pub(crate) fn replace(
    regex: &Regexp,
    data: Option<&ReplacerData>,
    mut evaluator: Option<MatchEvaluator<'_>>,
    input: &str,
    start_at: isize,
    count: isize,
) -> Result<String, Error> {
    let mut count = count;
    if count < -1 {
        return Err(Error::CountTooSmall);
    }
    if count == 0 {
        return Ok(String::new());
    }
    let Some(first) = regex.find_string_match_starting_at(input, start_at)? else {
        return Ok(input.to_string());
    };
    let text = first.text().clone();
    let mut buf = String::new();
    let mut m = Some(first);
    if !regex.right_to_left() {
        let mut prevat: isize = 0;
        while let Some(cur) = m {
            if cur.index != prevat {
                buf.push_str(&runes_to_string(&text[prevat as usize..cur.index as usize]));
            }
            prevat = cur.index + cur.length;
            match evaluator.as_mut() {
                None => replacement_impl(data.expect("replacement data"), &mut buf, &cur),
                Some(f) => buf.push_str(&f(&cur)),
            }
            count -= 1;
            if count == 0 {
                break;
            }
            m = match regex.find_next_match(&cur) {
                Ok(next) => next,
                Err(_) => return Ok(String::new()),
            };
        }
        if (prevat as usize) < text.len() {
            buf.push_str(&runes_to_string(&text[prevat as usize..]));
        }
    } else {
        let mut prevat = text.len() as isize;
        let mut al: Vec<String> = Vec::new();
        while let Some(cur) = m {
            if cur.index + cur.length != prevat {
                al.push(runes_to_string(
                    &text[(cur.index + cur.length) as usize..prevat as usize],
                ));
            }
            prevat = cur.index;
            match evaluator.as_mut() {
                None => replacement_impl_rtl(data.expect("replacement data"), &mut al, &cur),
                Some(f) => al.push(f(&cur)),
            }
            count -= 1;
            if count == 0 {
                break;
            }
            m = match regex.find_next_match(&cur) {
                Ok(next) => next,
                Err(_) => return Ok(String::new()),
            };
        }
        if prevat > 0 {
            buf.push_str(&runes_to_string(&text[..prevat as usize]));
        }
        for s in al.iter().rev() {
            buf.push_str(s);
        }
    }
    Ok(buf)
}

fn special(m: &Match<'_>, kind: isize, buf: &mut String) {
    let text = m.text();
    match kind {
        REPLACE_LEFT_PORTION => {
            for i in 0..m.index {
                push_rune(buf, text[i as usize]);
            }
        }
        REPLACE_RIGHT_PORTION => {
            for i in (m.index + m.length) as usize..text.len() {
                push_rune(buf, text[i]);
            }
        }
        REPLACE_LAST_GROUP => {
            m.group_value_append_to_buf(m.group_count() as isize - 1, buf);
        }
        REPLACE_WHOLE_STRING => {
            for &r in text.iter() {
                push_rune(buf, r);
            }
        }
        _ => {}
    }
}

fn replacement_impl(data: &ReplacerData, buf: &mut String, m: &Match<'_>) {
    for &r in &data.rules {
        if r >= 0 {
            buf.push_str(&data.strings[r as usize]);
        } else if r < -REPLACE_SPECIALS {
            m.group_value_append_to_buf(-REPLACE_SPECIALS - 1 - r, buf);
        } else {
            special(m, -REPLACE_SPECIALS - 1 - r, buf);
        }
    }
}

fn replacement_impl_rtl(data: &ReplacerData, al: &mut Vec<String>, m: &Match<'_>) {
    for &r in &data.rules {
        let mut buf = String::new();
        if r >= 0 {
            al.push(data.strings[r as usize].clone());
        } else if r < -REPLACE_SPECIALS {
            m.group_value_append_to_buf(-REPLACE_SPECIALS - 1 - r, &mut buf);
            al.push(buf);
        } else {
            special(m, -REPLACE_SPECIALS - 1 - r, &mut buf);
            al.push(buf);
        }
    }
}
