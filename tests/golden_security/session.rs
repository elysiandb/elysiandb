use std::path::Path;
use std::time::Duration;

use crate::common::{hex, read_jsonl, str_field, unhex};
use crate::{assert_manifest_entry, bool_of, security_store};
use elysiandb::crypto::bcrypt::{self, Hashed};
use elysiandb::crypto::sha256;
use elysiandb::gocompat::json::{self, Value};
use elysiandb::security::{self, BasicHashedUser, Role, SessionsFile};

const ROWS: usize = 11;
const GO_COUNTER: u64 = 89;

fn counter_block(n: u64) -> [u8; 32] {
    sha256::sum256(&n.to_le_bytes())
}

pub(crate) fn go_admin_hash(n: u64) -> String {
    let mut key = Vec::with_capacity(512);
    for j in 0..16 {
        key.extend_from_slice(&counter_block(n + j));
    }
    let key = hex(&key);

    let salt = bcrypt::base64_encode(&counter_block(n + 16)[..16]);

    let mut input = b"admin".to_vec();
    input.extend_from_slice(key.as_bytes());
    let digest = sha256::sum256(&input);

    let hash = bcrypt::bcrypt(&digest, 10, &salt).expect("bcrypt");
    let hashed = Hashed {
        hash,
        salt,
        cost: 10,
        major: b'2',
        minor: b'a',
    };
    String::from_utf8(hashed.hash()).expect("ascii hash")
}

fn mask(data: &[u8]) -> String {
    let text = String::from_utf8(data.to_vec()).expect("utf8 sessions file");
    let mut sf = SessionsFile::default();
    let _ = json::unmarshal(data, &mut sf);
    let mut masked = text;
    for s in sf.sessions.unwrap_or_default() {
        masked = masked.replace(&s.token_hash, "<id>");
        masked = masked.replace(&s.expires_at.to_string(), "<now+86400>");
    }
    masked
}

fn check_file(folder: &str, row: &Value) {
    let data =
        std::fs::read(Path::new(folder).join(security::SESSIONS_FILENAME)).expect("sessions file");
    assert_eq!(
        mask(&data),
        String::from_utf8(unhex(str_field(row, "sessions_file_hex"))).expect("utf8"),
        "{row:?}"
    );
}

#[test]
fn session_rows_match_go() {
    assert_manifest_entry("session.jsonl");
    let rows = read_jsonl("security/session.jsonl");
    let mut checked = 0;

    let mut files = Vec::new();
    let mut found = None;
    for row in &rows {
        match str_field(row, "fn") {
            "hashSessionID" => {
                assert_eq!(
                    security::hash_session_id(str_field(row, "id").as_bytes()),
                    str_field(row, "token_hash"),
                    "{row:?}"
                );
                checked += 1;
            }
            "credentialsFingerprint" => {
                assert_eq!(
                    security::credentials_fingerprint(str_field(row, "bcrypt").as_bytes()),
                    str_field(row, "credentials"),
                    "{row:?}"
                );
                checked += 1;
            }
            "sessions_file" => files.push(row),
            "GetSession" => found = Some(row),
            other => panic!("unknown function {other}"),
        }
    }

    let store = security_store("user");
    BasicHashedUser {
        username: b"admin".to_vec(),
        password: go_admin_hash(GO_COUNTER),
        role: Role::admin(),
    }
    .save()
    .expect("admin");

    let ttl = Duration::from_secs(24 * 3600);
    let s1 = security::create_session(b"admin", Role::admin(), ttl).expect("create");
    assert_eq!(str_field(files[0], "step"), "create");
    check_file(&store.path(), files[0]);
    checked += 1;

    let got = security::get_session(s1.id.as_bytes());
    let found = found.expect("GetSession row");
    assert_eq!(
        matches!(got, Ok(Some(_))),
        bool_of(found, "found"),
        "{found:?}"
    );
    checked += 1;

    security::delete_session(s1.id.as_bytes()).expect("delete");
    assert_eq!(str_field(files[1], "step"), "delete-last");
    check_file(&store.path(), files[1]);
    checked += 1;

    security::create_session(b"admin", Role::admin(), ttl).expect("second create");
    security::delete_user_sessions(b"admin").expect("delete user sessions");
    assert_eq!(str_field(files[2], "step"), "delete-user-sessions");
    check_file(&store.path(), files[2]);
    checked += 1;

    assert_eq!(files.len(), 3);
    assert_eq!(checked, ROWS);
}
