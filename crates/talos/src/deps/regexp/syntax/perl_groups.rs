use gocompat::utf8::Rune;

#[derive(Clone, Copy, Debug)]
pub struct CharGroup {
    pub sign: i32,
    pub class: &'static [Rune],
}

const CODE1: &[Rune] = &[0x30, 0x39];

const CODE2: &[Rune] = &[0x9, 0xa, 0xc, 0xd, 0x20, 0x20];

const CODE3: &[Rune] = &[0x30, 0x39, 0x41, 0x5a, 0x5f, 0x5f, 0x61, 0x7a];

pub static PERL_GROUP: &[(&[u8], CharGroup)] = &[
    (
        br"\d",
        CharGroup {
            sign: 1,
            class: CODE1,
        },
    ),
    (
        br"\D",
        CharGroup {
            sign: -1,
            class: CODE1,
        },
    ),
    (
        br"\s",
        CharGroup {
            sign: 1,
            class: CODE2,
        },
    ),
    (
        br"\S",
        CharGroup {
            sign: -1,
            class: CODE2,
        },
    ),
    (
        br"\w",
        CharGroup {
            sign: 1,
            class: CODE3,
        },
    ),
    (
        br"\W",
        CharGroup {
            sign: -1,
            class: CODE3,
        },
    ),
];

const CODE4: &[Rune] = &[0x30, 0x39, 0x41, 0x5a, 0x61, 0x7a];

const CODE5: &[Rune] = &[0x41, 0x5a, 0x61, 0x7a];

const CODE6: &[Rune] = &[0x0, 0x7f];

const CODE7: &[Rune] = &[0x9, 0x9, 0x20, 0x20];

const CODE8: &[Rune] = &[0x0, 0x1f, 0x7f, 0x7f];

const CODE9: &[Rune] = &[0x30, 0x39];

const CODE10: &[Rune] = &[0x21, 0x7e];

const CODE11: &[Rune] = &[0x61, 0x7a];

const CODE12: &[Rune] = &[0x20, 0x7e];

const CODE13: &[Rune] = &[0x21, 0x2f, 0x3a, 0x40, 0x5b, 0x60, 0x7b, 0x7e];

const CODE14: &[Rune] = &[0x9, 0xd, 0x20, 0x20];

const CODE15: &[Rune] = &[0x41, 0x5a];

const CODE16: &[Rune] = &[0x30, 0x39, 0x41, 0x5a, 0x5f, 0x5f, 0x61, 0x7a];

const CODE17: &[Rune] = &[0x30, 0x39, 0x41, 0x46, 0x61, 0x66];

pub static POSIX_GROUP: &[(&[u8], CharGroup)] = &[
    (
        b"[:alnum:]",
        CharGroup {
            sign: 1,
            class: CODE4,
        },
    ),
    (
        b"[:^alnum:]",
        CharGroup {
            sign: -1,
            class: CODE4,
        },
    ),
    (
        b"[:alpha:]",
        CharGroup {
            sign: 1,
            class: CODE5,
        },
    ),
    (
        b"[:^alpha:]",
        CharGroup {
            sign: -1,
            class: CODE5,
        },
    ),
    (
        b"[:ascii:]",
        CharGroup {
            sign: 1,
            class: CODE6,
        },
    ),
    (
        b"[:^ascii:]",
        CharGroup {
            sign: -1,
            class: CODE6,
        },
    ),
    (
        b"[:blank:]",
        CharGroup {
            sign: 1,
            class: CODE7,
        },
    ),
    (
        b"[:^blank:]",
        CharGroup {
            sign: -1,
            class: CODE7,
        },
    ),
    (
        b"[:cntrl:]",
        CharGroup {
            sign: 1,
            class: CODE8,
        },
    ),
    (
        b"[:^cntrl:]",
        CharGroup {
            sign: -1,
            class: CODE8,
        },
    ),
    (
        b"[:digit:]",
        CharGroup {
            sign: 1,
            class: CODE9,
        },
    ),
    (
        b"[:^digit:]",
        CharGroup {
            sign: -1,
            class: CODE9,
        },
    ),
    (
        b"[:graph:]",
        CharGroup {
            sign: 1,
            class: CODE10,
        },
    ),
    (
        b"[:^graph:]",
        CharGroup {
            sign: -1,
            class: CODE10,
        },
    ),
    (
        b"[:lower:]",
        CharGroup {
            sign: 1,
            class: CODE11,
        },
    ),
    (
        b"[:^lower:]",
        CharGroup {
            sign: -1,
            class: CODE11,
        },
    ),
    (
        b"[:print:]",
        CharGroup {
            sign: 1,
            class: CODE12,
        },
    ),
    (
        b"[:^print:]",
        CharGroup {
            sign: -1,
            class: CODE12,
        },
    ),
    (
        b"[:punct:]",
        CharGroup {
            sign: 1,
            class: CODE13,
        },
    ),
    (
        b"[:^punct:]",
        CharGroup {
            sign: -1,
            class: CODE13,
        },
    ),
    (
        b"[:space:]",
        CharGroup {
            sign: 1,
            class: CODE14,
        },
    ),
    (
        b"[:^space:]",
        CharGroup {
            sign: -1,
            class: CODE14,
        },
    ),
    (
        b"[:upper:]",
        CharGroup {
            sign: 1,
            class: CODE15,
        },
    ),
    (
        b"[:^upper:]",
        CharGroup {
            sign: -1,
            class: CODE15,
        },
    ),
    (
        b"[:word:]",
        CharGroup {
            sign: 1,
            class: CODE16,
        },
    ),
    (
        b"[:^word:]",
        CharGroup {
            sign: -1,
            class: CODE16,
        },
    ),
    (
        b"[:xdigit:]",
        CharGroup {
            sign: 1,
            class: CODE17,
        },
    ),
    (
        b"[:^xdigit:]",
        CharGroup {
            sign: -1,
            class: CODE17,
        },
    ),
];

fn lookup(table: &[(&[u8], CharGroup)], name: &[u8]) -> Option<CharGroup> {
    table.iter().find(|(n, _)| *n == name).map(|(_, g)| *g)
}

pub fn perl_group(name: &[u8]) -> Option<CharGroup> {
    lookup(PERL_GROUP, name)
}

pub fn posix_group(name: &[u8]) -> Option<CharGroup> {
    lookup(POSIX_GROUP, name)
}
