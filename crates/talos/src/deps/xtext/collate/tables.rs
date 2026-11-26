macro_rules! table {
    ($name:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/golden/hooks/xtext/collate/",
            $name,
            ".bin"
        ))
    };
}

pub const CLDR_VERSION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/golden/hooks/xtext/collate/CLDRVersion.bin"
));

pub const UNICODE_VERSION: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/golden/hooks/xtext/collate/UnicodeVersion.bin"
));

pub(super) static LOCALES: &[u8] = table!("locales");
pub(super) static MAIN_CT_ENTRIES: &[u8] = table!("mainCTEntries");
pub(super) static MAIN_CONTRACT_ELEM: &[u8] = table!("mainContractElem");
pub(super) static MAIN_EXPAND_ELEM: &[u8] = table!("mainExpandElem");
pub(super) static MAIN_LOOKUP: &[u8] = table!("mainLookup");
pub(super) static MAIN_VALUES: &[u8] = table!("mainValues");

pub(super) const VAR_TOP: u32 = 782;
