use std::sync::Arc;

use crate::common::{read_jsonl, str_field};
use crate::{assert_manifest_entry, security_store};
use elysiandb::acl::{self, Acl};
use elysiandb::globals;
use elysiandb::gocompat::json;
use elysiandb::security::{self, BasicUser, Role};

const ROWS: usize = 5;

fn null_when_empty<T>(list: Vec<T>) -> Option<Vec<T>> {
    if list.is_empty() { None } else { Some(list) }
}

#[test]
fn acl_rows_match_go() {
    assert_manifest_entry("acl.jsonl");
    let rows = read_jsonl("security/acl.jsonl");
    let mut checked = 0;

    for row in rows.iter().filter(|r| str_field(r, "fn") == "default") {
        let a = Acl {
            username: b"u".to_vec(),
            entity: b"book".to_vec(),
            permissions: acl::default_permissions_for_role(&Role::new(str_field(row, "role"))),
        };
        let encoded = json::marshal(&a).expect("marshal");
        assert_eq!(
            String::from_utf8_lossy(&encoded),
            str_field(row, "acl_json"),
            "{row:?}"
        );
        checked += 1;
    }

    let _store = security_store("user");
    let mut cfg = globals::config().map(|c| (*c).clone()).unwrap_or_default();
    cfg.api.schema.enabled = true;
    cfg.api.hooks.enabled = true;
    cfg.stats.enabled = true;
    globals::set_config(Some(Arc::new(cfg)));
    security::init_admin_user_if_not_exists().expect("admin");
    acl::init_acl().expect("init acl");

    let generated = rows
        .iter()
        .find(|r| str_field(r, "fn") == "GenerateACLFoAllrEntities")
        .expect("generate row");
    let list = null_when_empty(acl::generate_acl_fo_allr_entities());
    assert_eq!(
        String::from_utf8_lossy(&json::marshal(&list).expect("marshal")),
        str_field(generated, "zero_types"),
        "{generated:?}"
    );
    checked += 1;

    security::create_basic_user(&BasicUser {
        username: b"ann".to_vec(),
        password: b"secret-1".to_vec(),
        role: Role::user(),
    })
    .expect("ann");
    let lookup = rows
        .iter()
        .find(|r| str_field(r, "fn") == "GetACLEntityForUsername")
        .expect("lookup row");
    let found = acl::get_acl_entity_for_username(
        str_field(lookup, "entity").as_bytes(),
        str_field(lookup, "username").as_bytes(),
    );
    assert_eq!(
        String::from_utf8_lossy(&json::marshal(&found).expect("marshal")),
        str_field(lookup, "acl_json"),
        "{lookup:?}"
    );
    checked += 1;

    assert_eq!(checked, ROWS);
    assert_eq!(rows.len(), ROWS);
}
