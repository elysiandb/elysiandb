use elysiandb::api;
use elysiandb::gocompat::json::Value;
use elysiandb::hook::{self, Hook};
use goja::host::DocValue;

use crate::{doc, entity, entity_of, field, setup, text};

#[test]
fn test_get_default_hook_script_js_for_post_read() {
    let _s = setup(true);

    let s = hook::get_default_hook_script_js_for_post_read();
    assert!(!s.is_empty(), "expected non-empty default script");
    assert!(s.len() >= 10, "expected default script to be longer");
}

#[test]
fn test_get_default_hook_script_js_for_pre_read() {
    let _s = setup(true);

    let s = hook::get_default_hook_script_js_for_pre_read();
    assert!(!s.is_empty(), "expected non-empty default script");
    assert!(s.len() >= 10, "expected default script to be longer");
}

#[test]
fn test_hook_to_data_map_and_from_data_map() {
    let _s = setup(true);

    let h = Hook {
        id: "id1".to_string(),
        name: "name".to_string(),
        script: "script".to_string(),
        entity: "toto".to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        priority: 7,
        bypass_acl: true,
        enabled: true,
    };

    let mut m = h.to_data_map();

    let mut out = Hook::default();
    out.from_data_map(&m);
    assert_eq!(out, h, "roundtrip mismatch");

    m.insert("priority".to_string(), Value::Number(12.0));
    let mut out2 = Hook::default();
    out2.from_data_map(&m);
    assert_eq!(out2.priority, 12, "expected priority 12");

    m.insert("priority".to_string(), Value::Int(9));
    let mut out3 = Hook::default();
    out3.from_data_map(&m);
    assert_eq!(out3.priority, 9, "expected priority 9");
}

#[test]
fn test_init_hooks_disabled_does_nothing() {
    let _s = setup(false);

    hook::init_hooks();
    assert!(
        !api::entity_type_exists(hook::HOOK_ENTITY.as_bytes()),
        "did not expect hook entity type to exist when hooks disabled"
    );
}

#[test]
fn test_init_hooks_creates_entity_type() {
    let _s = setup(true);

    assert!(
        !api::entity_type_exists(hook::HOOK_ENTITY.as_bytes()),
        "expected hook entity type to not exist yet"
    );

    hook::init_hooks();

    assert!(
        api::entity_type_exists(hook::HOOK_ENTITY.as_bytes()),
        "expected hook entity type to exist after init"
    );
}

#[test]
fn test_create_hook_entity() {
    let _s = setup(true);

    assert!(
        !api::entity_type_exists(hook::HOOK_ENTITY.as_bytes()),
        "expected hook entity type to not exist yet"
    );

    hook::create_hook_entity().expect("expected no error");

    assert!(
        api::entity_type_exists(hook::HOOK_ENTITY.as_bytes()),
        "expected hook entity type to exist"
    );
}

fn missing_entry_is_noop(
    fn_apply: fn(&str, &str, &goja::host::MapRef, bool) -> Result<(), hook::HookError>,
) {
    let _s = setup(true);

    let e = entity_of(vec![
        ("id", DocValue::string("1")),
        ("x", DocValue::Int64(0)),
    ]);
    let script = "function other(ctx){ return ctx.entity }";
    if let Err(err) = fn_apply("", script, &e, true) {
        panic!("expected no error, got {err}");
    }
    assert!(
        matches!(field(&e, "x"), DocValue::Int64(0)),
        "expected x unchanged"
    );
}

fn syntax_error(
    fn_apply: fn(&str, &str, &goja::host::MapRef, bool) -> Result<(), hook::HookError>,
    script: &str,
) {
    let _s = setup(true);

    let e = entity(r#"{"id":"1"}"#);
    assert!(fn_apply("", script, &e, true).is_err(), "expected error");
}

fn query_works(
    fn_apply: fn(&str, &str, &goja::host::MapRef, bool) -> Result<(), hook::HookError>,
    script: &str,
) {
    let _s = setup(true);

    api::create_entity_type(b"order").expect("create order entity type");

    api::write_entity(b"order", &mut doc(r#"{"id":"o1","totoId":"1"}"#));
    api::write_entity(b"order", &mut doc(r#"{"id":"o2","totoId":"1"}"#));
    api::write_entity(b"order", &mut doc(r#"{"id":"o3","totoId":"2"}"#));

    let e = entity(r#"{"id":"1"}"#);

    if let Err(err) = fn_apply("", script, &e, true) {
        panic!("expected no error, got {err}");
    }

    assert!(
        matches!(field(&e, "ordersCount"), DocValue::Int64(2)),
        "expected ordersCount to be 2"
    );
    assert!(
        matches!(field(&e, "ok"), DocValue::Bool(true)),
        "expected ok=true"
    );
}

#[test]
fn test_apply_post_read_script_missing_post_read_is_noop() {
    missing_entry_is_noop(hook::apply_post_read_script);
}

#[test]
fn test_apply_post_read_script_syntax_error() {
    syntax_error(hook::apply_post_read_script, "function postRead(ctx) {");
}

#[test]
fn test_apply_post_read_script_query_arg_error() {
    syntax_error(
        hook::apply_post_read_script,
        "\nfunction postRead(ctx) {\n  ctx.query(\"order\")\n  return ctx.entity\n}",
    );
}

#[test]
fn test_apply_post_read_script_mutates_entity_and_query_works() {
    query_works(
        hook::apply_post_read_script,
        "\nfunction postRead(ctx) {\n  const orders = ctx.query(\"order\", { totoId: { eq: ctx.entity.id } })\n  ctx.entity.ordersCount = orders.length\n  ctx.entity.ok = true\n  return ctx.entity\n}",
    );
}

#[test]
fn test_create_update_delete_get_hook_and_queries() {
    let _s = setup(true);

    hook::init_hooks();

    let mut h = Hook {
        entity: "toto".to_string(),
        name: "h1".to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        priority: 1,
        enabled: true,
        bypass_acl: false,
        script: String::new(),
        ..Hook::default()
    };

    hook::create_hook(&mut h).expect("create hook");

    assert!(
        hook::entity_has_hooks("toto"),
        "expected EntityHasHooks to be true"
    );

    let list = hook::get_hooks_for_entity("toto").expect("GetHooksForEntity");
    assert_eq!(list.len(), 1, "expected 1 hook");
    let id = list[0].id.clone();
    assert!(!id.is_empty(), "expected generated id");
    assert!(
        !list[0].script.is_empty(),
        "expected default script to be set"
    );

    let mut got = hook::get_hook_by_id(&id).expect("GetHookById");
    assert_eq!(got.id, id);

    got.name = "h1b".to_string();
    got.priority = 3;
    got.enabled = false;

    hook::update_hook(&mut got).expect("UpdateHook");

    let updated = hook::get_hook_by_id(&id).expect("GetHookById after update");
    assert!(
        updated.name == "h1b"
            && updated.priority == 3
            && !updated.enabled
            && updated.entity == "toto",
        "expected the update to be stored, got {updated:?}"
    );

    hook::delete_hook(&id).expect("DeleteHook");

    assert!(
        !hook::entity_has_hooks("toto"),
        "expected EntityHasHooks to be false after delete"
    );

    assert!(
        hook::get_hook_by_id(&id).is_err(),
        "expected error for missing hook"
    );

    assert!(
        hook::delete_hook(&id).is_err(),
        "expected error when deleting missing hook"
    );

    let mut missing = Hook {
        id: "missing".to_string(),
        name: "missing".to_string(),
        ..Hook::default()
    };
    assert!(
        hook::update_hook(&mut missing).is_err(),
        "expected error when updating missing hook"
    );
}

#[test]
fn test_create_hook_duplicate_id() {
    let _s = setup(true);

    hook::init_hooks();

    let id = "same-id";

    let mut h1 = Hook {
        id: id.to_string(),
        entity: "toto".to_string(),
        name: "h1".to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        script: "function postRead(ctx){ return ctx.entity }".to_string(),
        enabled: true,
        priority: 1,
        ..Hook::default()
    };

    hook::create_hook(&mut h1).expect("create hook");

    let mut h2 = Hook {
        id: id.to_string(),
        entity: "toto".to_string(),
        name: "h2".to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        script: "function postRead(ctx){ return ctx.entity }".to_string(),
        enabled: true,
        priority: 2,
        ..Hook::default()
    };

    assert!(
        hook::create_hook(&mut h2).is_err(),
        "expected error for duplicate id"
    );
}

fn new_hook(name: &str, event: &str, script: &str, enabled: bool, priority: i64) -> Hook {
    Hook {
        entity: "toto".to_string(),
        name: name.to_string(),
        event: event.to_string(),
        language: "javascript".to_string(),
        script: script.to_string(),
        enabled,
        priority,
        ..Hook::default()
    }
}

#[test]
fn test_get_post_read_hooks_for_entity_filters_event() {
    let _s = setup(true);

    hook::init_hooks();

    let _ = hook::create_hook(&mut new_hook(
        "a",
        hook::HOOK_EVENT_POST_READ,
        "function postRead(ctx){ ctx.entity.a = 1; return ctx.entity }",
        true,
        1,
    ));

    let _ = hook::create_hook(&mut new_hook(
        "b",
        "other_event",
        "function postRead(ctx){ ctx.entity.b = 1; return ctx.entity }",
        true,
        1,
    ));

    let list = hook::get_post_read_hooks_for_entity("toto");
    assert_eq!(list.len(), 1, "expected 1 post_read hook");
    assert_eq!(list[0].event, hook::HOOK_EVENT_POST_READ);
}

fn priority_enabled_and_error_tolerance(
    event: &str,
    fn_name: &str,
    apply: fn(&str, &str, &goja::host::MapRef) -> goja::host::MapRef,
) {
    let _s = setup(true);

    hook::init_hooks();

    let step = |s: &str| {
        format!(
            "function {fn_name}(ctx){{ ctx.entity.steps = (ctx.entity.steps || '') + '{s}'; return ctx.entity }}"
        )
    };

    let _ = hook::create_hook(&mut new_hook("high", event, &step("A"), true, 10));
    let _ = hook::create_hook(&mut new_hook("low", event, &step("B"), true, 1));
    let _ = hook::create_hook(&mut new_hook("disabled", event, &step("X"), false, 100));
    let _ = hook::create_hook(&mut new_hook(
        "broken",
        event,
        &format!("function {fn_name}(ctx){{ throw new Error('boom') }}"),
        true,
        5,
    ));

    let e = entity(r#"{"id":"1"}"#);

    let out = apply("", "toto", &e);

    let steps = text(&out, "steps").unwrap_or_default();
    assert!(steps == "AB", "expected steps to be AB, got {steps:?}");
}

#[test]
fn test_apply_post_read_hooks_for_entity_priority_enabled_and_error_tolerance() {
    priority_enabled_and_error_tolerance(
        hook::HOOK_EVENT_POST_READ,
        "postRead",
        hook::apply_post_read_hooks_for_entity,
    );
}

#[test]
fn test_get_pre_read_hooks_for_entity_filters_event() {
    let _s = setup(true);

    hook::init_hooks();

    let _ = hook::create_hook(&mut new_hook(
        "a",
        hook::HOOK_EVENT_PRE_READ,
        "function preRead(ctx){ return ctx.entity }",
        true,
        1,
    ));

    let _ = hook::create_hook(&mut new_hook(
        "b",
        hook::HOOK_EVENT_POST_READ,
        "function postRead(ctx){ return ctx.entity }",
        true,
        1,
    ));

    let list = hook::get_pre_read_hooks_for_entity("toto");
    assert_eq!(list.len(), 1, "expected 1 pre_read hook");
    assert_eq!(list[0].event, hook::HOOK_EVENT_PRE_READ);
}

#[test]
fn test_apply_pre_read_hooks_for_entity_priority_enabled_and_error_tolerance() {
    priority_enabled_and_error_tolerance(
        hook::HOOK_EVENT_PRE_READ,
        "preRead",
        hook::apply_pre_read_hooks_for_entity,
    );
}

#[test]
fn test_apply_pre_read_script_missing_pre_read_is_noop() {
    missing_entry_is_noop(hook::apply_pre_read_script);
}

#[test]
fn test_apply_pre_read_script_syntax_error() {
    syntax_error(hook::apply_pre_read_script, "function preRead(ctx) {");
}

#[test]
fn test_apply_pre_read_script_query_arg_error() {
    syntax_error(
        hook::apply_pre_read_script,
        "\nfunction preRead(ctx) {\n  ctx.query(\"order\")\n  return ctx.entity\n}",
    );
}

#[test]
fn test_apply_pre_read_script_mutates_entity_and_query_works() {
    query_works(
        hook::apply_pre_read_script,
        "\nfunction preRead(ctx) {\n  const orders = ctx.query(\"order\", { totoId: { eq: ctx.entity.id } })\n  ctx.entity.ordersCount = orders.length\n  ctx.entity.ok = true\n  return ctx.entity\n}",
    );
}

#[test]
fn test_hook_validate_requires_core_fields() {
    let valid = Hook {
        name: "n".to_string(),
        entity: "e".to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        script: "function postRead(ctx) {}".to_string(),
        ..Hook::default()
    };
    if let Err(err) = valid.validate() {
        panic!("expected a valid hook, got {err}");
    }

    for f in ["name", "entity", "event", "language", "script"] {
        let mut h = valid.clone();
        match f {
            "name" => h.name.clear(),
            "entity" => h.entity.clear(),
            "event" => h.event.clear(),
            "language" => h.language.clear(),
            _ => h.script.clear(),
        }

        match h.validate() {
            Err(err) if err.to_string().contains(&format!("'{f}'")) => {}
            other => panic!("expected {f} to be required, got {other:?}"),
        }
    }
}

#[test]
fn test_update_hook_restores_default_script_and_rejects_invalid_hooks() {
    let _s = setup(true);
    hook::init_hooks();

    let mut h = Hook {
        entity: "toto".to_string(),
        name: "h".to_string(),
        event: hook::HOOK_EVENT_PRE_READ.to_string(),
        language: "javascript".to_string(),
        enabled: true,
        ..Hook::default()
    };
    hook::create_hook(&mut h).expect("create");

    h.script.clear();
    hook::update_hook(&mut h).expect("UpdateHook");

    let stored = hook::get_hook_by_id(&h.id).expect("stored");
    assert_eq!(
        stored.script,
        hook::get_default_hook_script_js_for_pre_read(),
        "expected the default script for the event"
    );

    h.entity.clear();
    assert!(
        hook::update_hook(&mut h).is_err(),
        "expected a hook without entity to be refused"
    );

    let stored = hook::get_hook_by_id(&h.id).expect("stored");
    assert_eq!(
        stored.entity, "toto",
        "a refused update must not be stored, got {stored:?}"
    );
}

#[test]
fn test_update_hook_rejects_unknown_hooks_and_events() {
    let _s = setup(true);
    hook::init_hooks();

    let mut missing = Hook {
        id: "missing".to_string(),
        name: "m".to_string(),
        entity: "toto".to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        script: "function postRead(ctx) {}".to_string(),
        ..Hook::default()
    };
    match hook::update_hook(&mut missing) {
        Err(err) if err.to_string().contains("does not exist") => {}
        other => panic!("expected an unknown hook to be refused, got {other:?}"),
    }

    let mut h = Hook {
        entity: "toto".to_string(),
        name: "h".to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        ..Hook::default()
    };
    hook::create_hook(&mut h).expect("create");

    h.event = "on_write".to_string();
    h.script.clear();
    match hook::update_hook(&mut h) {
        Err(err) if err.to_string().contains("'script'") => {}
        other => panic!("expected an empty script without default to be refused, got {other:?}"),
    }
}
