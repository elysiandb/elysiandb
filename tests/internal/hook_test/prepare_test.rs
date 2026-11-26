use std::time::{Duration, Instant};

use elysiandb::api;
use elysiandb::datastore::{IndexState, ORIGIN_HOOK};
use elysiandb::gocompat::time::Duration as GoDuration;
use elysiandb::hook::{self, Hook};
use elysiandb::storage;

use crate::{entity, fmt_field, obj, s, setup, write};

fn wait_for_index(entity: &str, name: &str) -> IndexState {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        for st in storage::engine().expect("engine").index_states() {
            if st.entity == entity.as_bytes() && st.index == name.as_bytes() && st.state == "ready"
            {
                return st;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let states: Vec<String> = storage::engine()
        .expect("engine")
        .index_states()
        .iter()
        .map(|st| {
            format!(
                "{}:{}:{}",
                String::from_utf8_lossy(&st.entity),
                String::from_utf8_lossy(&st.index),
                st.state
            )
        })
        .collect();
    panic!("index {name} on {entity} never became ready, states {states:?}");
}

#[test]
fn hooks_get_their_query_indexes_in_advance_and_keep_them() {
    let _s = setup(true);
    hook::init_hooks();

    api::create_entity_type(b"step").expect("create step");
    for i in 0..50 {
        write(
            "step",
            obj(vec![
                ("id", s(&format!("s{i:02}"))),
                ("program_id", s(&format!("p{}", i % 5))),
                ("status", s("vu")),
            ]),
        );
    }

    let mut h = Hook {
        name: "progress".to_string(),
        entity: "program".to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        enabled: true,
        bypass_acl: true,
        script: r#"function postRead(ctx) {
  var steps = ctx.query("step", { program_id: { eq: ctx.entity.id }, "status": { neq: "x" } })
  ctx.entity.n = steps.length
}"#
        .to_string(),
        ..Hook::default()
    };
    hook::create_hook(&mut h).expect("create hook");

    for name in ["f:program_id", "f:status"] {
        let st = wait_for_index("step", name);
        assert!(
            st.origin == ORIGIN_HOOK.as_bytes() && st.pinned,
            "{name} should be a pinned hook index"
        );
    }

    let e = storage::engine().expect("engine");
    if e.drop_idle_indexes(GoDuration(0)) != 0 {
        for st in e.index_states() {
            assert!(
                !(st.entity == b"step" && st.state != "ready"),
                "a hook index was released while its hook exists"
            );
        }
    }

    hook::delete_hook(&h.id).expect("delete hook");

    for st in storage::engine().expect("engine").index_states() {
        assert!(
            !(st.entity == b"step" && st.pinned),
            "deleting the hook should unpin its indexes"
        );
    }
}

#[test]
fn dynamic_hook_queries_are_prepared_at_first_run() {
    let _s = setup(true);
    hook::init_hooks();

    api::create_entity_type(b"rows").expect("create rows");
    write("rows", obj(vec![("id", s("r1")), ("group", s("g"))]));

    let mut h = Hook {
        name: "dynamic".to_string(),
        entity: "parent".to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        enabled: true,
        bypass_acl: true,
        script: r#"function postRead(ctx) {
  var field = "gr" + "oup"
  var filter = {}
  filter[field] = { eq: "g" }
  ctx.entity.n = ctx.query("rows", filter).length
}"#
        .to_string(),
        ..Hook::default()
    };
    hook::create_hook(&mut h).expect("create hook");

    let out = hook::apply_post_read_hooks_for_entity("", "parent", &entity(r#"{"id":"x"}"#));
    assert_eq!(fmt_field(&out, "n"), "1", "hook result");

    let st = wait_for_index("rows", "f:group");
    assert!(
        st.pinned,
        "an index seen at the first run should be pinned for the hook"
    );
}
