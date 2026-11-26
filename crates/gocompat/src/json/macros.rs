#[macro_export]
macro_rules! json_struct {
    (@name -) => {
        ::core::option::Option::None
    };
    (@name $name:literal) => {
        ::core::option::Option::Some($name)
    };

    (@dec $d:ident, $index:ident, $pos:ident, $field:expr, -, [$($opt:ident)*]) => {
        $pos += 1;
    };
    (@dec $d:ident, $index:ident, $pos:ident, $field:expr, $name:literal, [$($opt:ident)*]) => {
        if $index == $pos {
            return $crate::json_struct!(@dec_opts $d, $field, [$($opt)*]);
        }
        $pos += 1;
    };
    (@dec_opts $d:ident, $field:expr, []) => {
        $crate::json::Decode::decode(&mut $field, $d)
    };
    (@dec_opts $d:ident, $field:expr, [string $($rest:ident)*]) => {
        $crate::json::decode_quoted(&mut $field, $d)
    };
    (@dec_opts $d:ident, $field:expr, [omitempty $($rest:ident)*]) => {
        $crate::json_struct!(@dec_opts $d, $field, [$($rest)*])
    };

    (@enc $e:ident, $first:ident, $field:expr, -, [$($opt:ident)*]) => {};
    (@enc $e:ident, $first:ident, $field:expr, $name:literal, [$($opt:ident)*]) => {
        $crate::json_struct!(@enc_opts $e, $first, $field, $name, false, false, [$($opt)*]);
    };
    (@enc_opts $e:ident, $first:ident, $field:expr, $name:literal, $omit:tt, $quoted:tt, [omitempty $($rest:ident)*]) => {
        $crate::json_struct!(@enc_opts $e, $first, $field, $name, true, $quoted, [$($rest)*]);
    };
    (@enc_opts $e:ident, $first:ident, $field:expr, $name:literal, $omit:tt, $quoted:tt, [string $($rest:ident)*]) => {
        $crate::json_struct!(@enc_opts $e, $first, $field, $name, $omit, true, [$($rest)*]);
    };
    (@enc_opts $e:ident, $first:ident, $field:expr, $name:literal, $omit:tt, $quoted:tt, []) => {
        $crate::json_struct!(@enc_emit $e, $first, $field, $name, $omit, $quoted);
    };
    (@enc_emit $e:ident, $first:ident, $field:expr, $name:literal, false, false) => {
        $e.write_field_name(&mut $first, $name);
        $crate::json::Encode::encode(&$field, $e)?;
    };
    (@enc_emit $e:ident, $first:ident, $field:expr, $name:literal, true, false) => {
        if !$crate::json::Encode::is_empty_value(&$field) {
            $e.write_field_name(&mut $first, $name);
            $crate::json::Encode::encode(&$field, $e)?;
        }
    };
    (@enc_emit $e:ident, $first:ident, $field:expr, $name:literal, false, true) => {
        $e.write_field_name(&mut $first, $name);
        $crate::json::EncodeQuoted::encode_quoted(&$field, $e)?;
    };
    (@enc_emit $e:ident, $first:ident, $field:expr, $name:literal, true, true) => {
        if !$crate::json::Encode::is_empty_value(&$field) {
            $e.write_field_name(&mut $first, $name);
            $crate::json::EncodeQuoted::encode_quoted(&$field, $e)?;
        }
    };

    (
        $(#[$attr:meta])*
        $vis:vis struct $name:ident {
            $(
                $fvis:vis $fname:ident : $fty:ty => $jname:tt $([$($opt:ident),* $(,)?])?
            ),* $(,)?
        }
    ) => {
        $(#[$attr])*
        $vis struct $name {
            $($fvis $fname: $fty),*
        }

        impl $crate::json::Decode for $name {
            const EXPECTED: $crate::json::Expected = $crate::json::Expected::Object;

            fn decode(
                &mut self,
                d: &mut $crate::json::DecodeState<'_>,
            ) -> ::core::result::Result<(), $crate::json::Error> {
                const NAMES: &[::core::option::Option<&str>] = &[$($crate::json_struct!(@name $jname)),*];
                let this = self;
                d.decode_struct(NAMES, |d, index| {
                    let mut position = 0usize;
                    $($crate::json_struct!(@dec d, index, position, this.$fname, $jname, [$($($opt)*)?]);)*
                    let _ = position;
                    ::core::result::Result::Ok(())
                })
            }
        }

        impl $crate::json::Encode for $name {
            fn encode(
                &self,
                e: &mut $crate::json::EncodeState,
            ) -> ::core::result::Result<(), $crate::json::Error> {
                let mut first = true;
                $($crate::json_struct!(@enc e, first, self.$fname, $jname, [$($($opt)*)?]);)*
                e.end_struct(first);
                ::core::result::Result::Ok(())
            }
        }
    };
}
