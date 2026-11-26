mod composition;
#[cfg(test)]
mod composition_test;
#[cfg(test)]
mod example_test;
mod forminfo;
#[cfg(test)]
mod forminfo_test;
mod iter;
#[cfg(test)]
mod iter_test;
mod normalize;
#[cfg(test)]
mod normalize_test;
mod tables;
pub mod transform;
mod trie;

pub use composition::GRAPHEME_JOINER;
pub use forminfo::Properties;
pub use iter::{Iter, MAX_SEGMENT_SIZE};
pub use normalize::Form;
pub use tables::{MAX_TRANSFORM_CHUNK_SIZE, VERSION};
