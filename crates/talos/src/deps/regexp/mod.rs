pub mod backtrack;
pub mod exec;
pub mod onepass;
#[allow(clippy::module_inception)]
pub mod regexp;
pub mod syntax;

pub use self::backtrack::{MAX_BACKTRACK_PROG, MAX_BACKTRACK_VECTOR};
pub use self::onepass::{
    MERGE_FAILED, OnePassInst, OnePassProg, compile_one_pass, merge_rune_sets,
};
pub use self::regexp::{
    Regexp, RuneReader, compile, compile_mode, compile_posix, r#match, match_reader, match_string,
    min_input_len, must_compile, must_compile_posix, quote_meta,
};
