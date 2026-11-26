use elysiandb::api;
use elysiandb::gocompat::json::{Map, Value};

use super::{doc, setup};

pub fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, &c)| match i {
            8 | 13 | 18 | 23 => c == b'-',
            _ => c.is_ascii_digit() || (b'a'..=b'f').contains(&c),
        })
        && b[14] == b'4'
        && matches!(b[19], b'8' | b'9' | b'a' | b'b')
}

fn text<'a>(m: &'a Map, key: &str) -> Option<&'a str> {
    m.get(key).and_then(Value::as_str)
}

#[test]
fn extract_sub_entities_simple() {
    let _s = setup();
    let mut data = doc(r#"{"name":"youyou","author":{"@entity":"author","fullname":"Taymour"}}"#);
    let subs = api::extract_sub_entities(b"article", &mut data);
    if subs.len() != 1 {
        panic!("expected 1 sub entity, got {}", subs.len());
    }
    if text(&subs[0], "@entity") != Some("author") {
        panic!(
            "expected sub entity 'author', got {:?}",
            subs[0].get("@entity")
        );
    }
    if text(&subs[0], "id").is_none() {
        panic!("expected id field in sub entity");
    }
    let Some(Value::Object(a)) = data.get("author") else {
        panic!("expected author link");
    };
    if text(a, "@entity") != Some("author") {
        panic!("expected author link to have @entity");
    }
    if text(a, "id").is_none() {
        panic!("expected author link to have id");
    }
}

#[test]
fn extract_sub_entities_nested() {
    let _s = setup();
    let mut data = doc(
        r#"{"name":"youyou","author":{"@entity":"author","fullname":"Taymour Negib","status":"writer","job":{"@entity":"job","designation":"Worker"}}}"#,
    );
    let subs = api::extract_sub_entities(b"article", &mut data);
    if subs.len() != 2 {
        panic!("expected 2 sub entities, got {}", subs.len());
    }
    let author = subs.iter().find(|s| text(s, "@entity") == Some("author"));
    let job = subs.iter().find(|s| text(s, "@entity") == Some("job"));
    let (Some(author), Some(job)) = (author, job) else {
        panic!("missing author or job in subs");
    };
    if text(job, "designation") != Some("Worker") {
        panic!(
            "expected job.designation=Worker, got {:?}",
            job.get("designation")
        );
    }
    let Some(Value::Object(j)) = author.get("job") else {
        panic!("expected author.job to be a map");
    };
    if text(j, "@entity") != Some("job") {
        panic!(
            "expected author.job.@entity=job, got {:?}",
            j.get("@entity")
        );
    }
    if !text(j, "id").is_some_and(is_uuid) {
        panic!("expected valid uuid for job.id");
    }
}

#[test]
fn extract_sub_entities_array() {
    let _s = setup();
    let mut data = doc(
        r#"{"name":"youyou","age":13,"title":"this is it","authors":[{"@entity":"author","fullname":"Taymour Negib","status":"writer","job":{"@entity":"job","designation":"Worker"}},{"@entity":"author","fullname":"Alberto","status":"coco"}]}"#,
    );
    let subs = api::extract_sub_entities(b"article", &mut data);
    if subs.len() != 3 {
        panic!(
            "expected 3 sub entities (2 authors + 1 job), got {}",
            subs.len()
        );
    }
    let authors: Vec<&Map> = subs
        .iter()
        .filter(|s| text(s, "@entity") == Some("author"))
        .collect();
    let job = subs.iter().find(|s| text(s, "@entity") == Some("job"));
    if authors.len() != 2 {
        panic!("expected 2 authors, got {}", authors.len());
    }
    let Some(job) = job else {
        panic!("missing job in subs");
    };
    if text(job, "designation") != Some("Worker") {
        panic!(
            "expected job.designation=Worker, got {:?}",
            job.get("designation")
        );
    }
    let Some(Value::Array(arr)) = data.get("authors") else {
        panic!("expected author links");
    };
    if arr.len() != 2 {
        panic!("expected 2 author links, got {}", arr.len());
    }
    for a in arr {
        let Value::Object(m) = a else {
            panic!("expected a link");
        };
        if text(m, "@entity") != Some("author") {
            panic!("expected @entity=author, got {:?}", m.get("@entity"));
        }
        if !text(m, "id").is_some_and(is_uuid) {
            panic!("expected valid uuid for author.id");
        }
    }
}

#[test]
fn extract_sub_entities_array_pure_reference() {
    let _s = setup();
    let mut data = doc(
        r#"{"label":"Box set","items":[{"@entity":"movie","id":"m1"},{"@entity":"movie","@id":"m2"},{"@entity":"movie","id":"m3","title":"Alien"}]}"#,
    );
    let subs = api::extract_sub_entities(b"boxset", &mut data);
    if subs.len() != 1 {
        panic!(
            "expected only the reference with fields to be extracted, got {} subs: {subs:?}",
            subs.len()
        );
    }
    if text(&subs[0], "id") != Some("m3") || text(&subs[0], "title") != Some("Alien") {
        panic!("unexpected sub entity: {:?}", subs[0]);
    }
    let Some(Value::Array(arr)) = data.get("items") else {
        panic!("expected links");
    };
    let expected = ["m1", "m2", "m3"];
    if arr.len() != expected.len() {
        panic!("expected {} links, got {}", expected.len(), arr.len());
    }
    for (i, a) in arr.iter().enumerate() {
        let want = doc(&format!(r#"{{"@entity":"movie","id":"{}"}}"#, expected[i]));
        if *a != Value::Object(want) {
            panic!("unexpected link at {i}: {a:?}");
        }
    }
}

#[test]
fn merge_with_existing() {
    let _s = setup();
    let sub = doc(r#"{"id":"m1","rating":10.0}"#);
    let got = api::merge_with_existing(sub.clone(), None);
    if got != sub {
        panic!("expected sub unchanged without existing, got {got:?}");
    }

    let existing = doc(r#"{"id":"m1","title":"Alien","rating":8.0}"#);
    let got = api::merge_with_existing(sub.clone(), Some(existing.clone()));
    let want = doc(r#"{"id":"m1","title":"Alien","rating":10.0}"#);
    if got != want {
        panic!("got {got:?}, want {want:?}");
    }
    if existing.get("rating") != Some(&Value::Number(8.0)) || sub.len() != 2 {
        panic!("inputs must not be modified, existing={existing:?} sub={sub:?}");
    }
}
