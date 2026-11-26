use elysiandb::globals;

#[test]
fn api_entity_key() {
    let tests = [
        ("users", "api:entity:users"),
        ("posts", "api:entity:posts"),
        ("weird-entity_name42", "api:entity:weird-entity_name42"),
        ("", "api:entity:"),
        ("team", "api:entity:team"),
    ];
    for (entity, want) in tests {
        let got = globals::api_entity_key(entity);
        assert_eq!(
            got, want,
            "ApiEntityKey({entity:?}) = {got:?}, want {want:?}"
        );
    }
}

#[test]
fn api_entities_all_key() {
    let tests = [
        ("users", "api:entity:users:*"),
        ("orders", "api:entity:orders:*"),
        ("", "api:entity::*"),
        ("νέα", "api:entity:νέα:*"),
    ];
    for (entity, want) in tests {
        let got = globals::api_entities_all_key(entity);
        assert_eq!(
            got, want,
            "ApiEntitiesAllKey({entity:?}) = {got:?}, want {want:?}"
        );
    }
}

#[test]
fn api_single_entity_key() {
    let tests = [
        ("users", "123", "api:entity:users:id:123"),
        ("articles", "a1b2c3", "api:entity:articles:id:a1b2c3"),
        ("", "", "api:entity::id:"),
        ("products", "Ω-λ-42", "api:entity:products:id:Ω-λ-42"),
    ];
    for (entity, id, want) in tests {
        let got = globals::api_single_entity_key(entity, id);
        assert_eq!(
            got, want,
            "ApiSingleEntityKey({entity:?},{id:?}) = {got:?}, want {want:?}"
        );
    }
}
