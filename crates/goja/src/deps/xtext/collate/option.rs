use crate::deps::xtext::colltab::{Level, NUM_LEVELS};
use crate::deps::xtext::language::Tag;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlternateHandling {
    #[default]
    NonIgnorable,
    Blanked,
    Shifted,
    ShiftTrimmed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Options {
    pub(super) ignore: [bool; NUM_LEVELS],
    pub(super) case_level: bool,
    pub(super) backwards: bool,
    pub(super) alternate: AlternateHandling,
}

impl Options {
    pub(super) fn new() -> Options {
        let mut ignore = [false; NUM_LEVELS];
        ignore[Level::Quaternary as usize] = true;
        ignore[Level::Identity as usize] = true;
        Options {
            ignore,
            ..Options::default()
        }
    }

    pub(super) fn set_options(&mut self, opts: &[Option]) {
        let mut opts = opts.to_vec();
        gocompat::sort::slice(&mut opts, |a, b| a.priority < b.priority);
        for x in &opts {
            (x.f)(self);
        }
    }

    pub(super) fn set_from_tag(&mut self, t: Tag) {
        self.case_level = ldml_bool(t, self.case_level, "kc");
        self.backwards = ldml_bool(t, self.backwards, "kb");
        match t.type_for_key("ks") {
            "level1" => {
                self.ignore[Level::Secondary as usize] = true;
                self.ignore[Level::Tertiary as usize] = true;
            }
            "level2" => {
                self.ignore[Level::Tertiary as usize] = true;
            }
            "level4" => {
                self.ignore[Level::Quaternary as usize] = false;
            }
            "identic" => {
                self.ignore[Level::Quaternary as usize] = false;
                self.ignore[Level::Identity as usize] = false;
            }
            _ => {}
        }
        match t.type_for_key("ka") {
            "shifted" => self.alternate = AlternateHandling::Shifted,
            "blanked" => self.alternate = AlternateHandling::Blanked,
            "posix" => self.alternate = AlternateHandling::ShiftTrimmed,
            _ => {}
        }
    }
}

fn ldml_bool(t: Tag, old: bool, key: &str) -> bool {
    match t.type_for_key(key) {
        "true" => true,
        "false" => false,
        _ => old,
    }
}

#[derive(Clone, Copy)]
pub struct Option {
    priority: i32,
    f: fn(&mut Options),
}

pub const IGNORE_CASE: Option = Option {
    priority: 3,
    f: ignore_case_f,
};

pub const IGNORE_DIACRITICS: Option = Option {
    priority: 3,
    f: ignore_diacritics_f,
};

pub const IGNORE_WIDTH: Option = Option {
    priority: 2,
    f: ignore_width_f,
};

pub const LOOSE: Option = Option {
    priority: 4,
    f: loose_f,
};

pub const FORCE: Option = Option {
    priority: 5,
    f: force_f,
};

fn ignore_width_f(o: &mut Options) {
    o.ignore[Level::Tertiary as usize] = true;
    o.case_level = true;
}

fn ignore_diacritics_f(o: &mut Options) {
    o.ignore[Level::Secondary as usize] = true;
}

fn ignore_case_f(o: &mut Options) {
    o.ignore[Level::Tertiary as usize] = true;
    o.case_level = false;
}

fn loose_f(o: &mut Options) {
    ignore_width_f(o);
    ignore_diacritics_f(o);
    ignore_case_f(o);
}

fn force_f(o: &mut Options) {
    o.ignore[Level::Identity as usize] = false;
}
