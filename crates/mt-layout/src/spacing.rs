//! The spacing table a layout run uses, in em units.
//!
//! [`crate::constants`] states the classic values every number here defaults to; a style preset
//! overrides any of them by name through [`Spacing::get`] and [`Spacing::set`], which use the
//! text keys a preset file spells ([`Spacing::KEYS`] lists them in field order).

use crate::constants::*;

/// Declares [`Spacing`]: one struct field, one text key and one default per row.
macro_rules! spacing_fields {
    ($(($key:literal, $field:ident, $default:expr)),* $(,)?) => {
        /// Every tunable spacing the layout engine places material with, in em units.
        ///
        /// The [`Default`] value of each field is the classic number [`crate::constants`] states,
        /// so a style built with defaults typesets exactly as the engine always has.
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct Spacing {
            $(
                #[doc = concat!("Text key `", $key, ".")]
                pub $field: f64,
            )*
        }

        impl Default for Spacing {
            fn default() -> Self {
                Self { $($field: $default,)* }
            }
        }

        impl Spacing {
            /// The preset-file keys, in field order.
            pub const KEYS: &'static [&'static str] = &[$($key),*];

            /// The value stored under `key`, or `None` when no field carries that key.
            pub fn get(&self, key: &str) -> Option<f64> {
                match key {
                    $($key => Some(self.$field),)*
                    _ => None,
                }
            }

            /// Store `value` under `key`; `false` when no field carries that key.
            pub fn set(&mut self, key: &str, value: f64) -> bool {
                match key {
                    $(
                        $key => {
                            self.$field = value;
                            true
                        }
                    )*
                    _ => false,
                }
            }
        }
    };
}

spacing_fields! {
    ("thin-space", thin_kern, THIN_KERN),
    ("medium-space", medium_kern, MEDIUM_KERN),
    ("thick-space", thick_kern, THICK_KERN),
    ("frac-rule-thickness", frac_rule_thickness, FRAC_RULE_THICKNESS),
    ("frac-gap", frac_rule_gap, FRAC_RULE_GAP),
    ("frac-nobar-gap", frac_nobar_gap, FRAC_NOBAR_GAP),
    ("frac-slash-shift", frac_slash_shift, FRAC_SLASH_SHIFT),
    ("frac-slash-kern", frac_slash_kern, FRAC_SLASH_KERN),
    ("radical-rule-thickness", radical_rule_thickness, RADICAL_RULE_THICKNESS),
    ("radical-gap", radical_rule_gap, RADICAL_RULE_GAP),
    ("radical-kern", radical_kern, RADICAL_KERN),
    ("radical-index-gap", radical_index_gap, RADICAL_INDEX_GAP),
    ("script-offset", script_offset, SCRIPT_OFFSET),
    ("subscript-offset", subscript_offset, SUBSCRIPT_OFFSET),
    ("script-kern", script_kern, SCRIPT_KERN),
    ("script-min-gap", script_min_gap, SCRIPT_MIN_GAP),
    ("limit-gap", limit_gap, LIMIT_GAP),
    ("fence-kern", fence_kern, FENCE_KERN),
    ("accent-gap", accent_gap, ACCENT_GAP),
    ("accent-rule-thickness", accent_rule_thickness, ACCENT_RULE_THICKNESS),
    ("matrix-col-gap", matrix_col_separation, MATRIX_COL_SEPARATION),
    ("matrix-rule-thickness", matrix_rule_thickness, MATRIX_RULE_THICKNESS),
}

#[cfg(test)]
mod tests {
    use super::Spacing;

    #[test]
    fn defaults_are_the_classic_constants() {
        let spacing = Spacing::default();
        assert_eq!(spacing.thin_kern, crate::constants::THIN_KERN);
        assert_eq!(spacing.thick_kern, crate::constants::THICK_KERN);
        assert_eq!(spacing.frac_rule_gap, crate::constants::FRAC_RULE_GAP);
        assert_eq!(spacing.matrix_col_separation, crate::constants::MATRIX_COL_SEPARATION);
    }

    #[test]
    fn keys_round_every_field() {
        let mut spacing = Spacing::default();
        assert_eq!(Spacing::KEYS.len(), 22);
        for (index, key) in Spacing::KEYS.iter().enumerate() {
            let value = 0.1 + index as f64;
            assert!(spacing.set(key, value), "{key} must name a field");
            assert_eq!(spacing.get(key), Some(value), "{key} must read back");
        }
        assert!(!spacing.set("unknown-key", 1.0));
        assert_eq!(spacing.get("unknown-key"), None);
    }
}
