//! The size and spacing preset layout is run with.

/// Size preset for one layout run.
///
/// A style carries the full size and the two scale factors used for nested material; the nesting
/// level of a piece of the equation is tracked separately, so the same style can be reused for
/// every slot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutStyle {
    /// Font size in points at full size.
    pub size: f64,
    /// Size factor for material one level deep, e.g. scripts and fraction arguments.
    pub script_scale: f64,
    /// Size factor for material two or more levels deep.
    pub script_script_scale: f64,
    /// Whether superscripts are set without the extra clearance that uncramped math gets.
    pub cramped: bool,
    /// Nesting level the equation itself starts at; 0 is full size.
    pub level: u8,
}

impl LayoutStyle {
    /// A full-size style of `size` points, starting at nesting level zero.
    pub fn display(size: f64) -> Self {
        Self { size, ..Self::default() }
    }

    /// Size factor applied at `level`: full at 0, `script_scale` at 1, `script_script_scale`
    /// from 2 onwards.
    pub fn scale(&self, level: u8) -> f64 {
        match level {
            0 => 1.0,
            1 => self.script_scale,
            _ => self.script_script_scale,
        }
    }

    /// Font size in points at `level`.
    pub fn size_at(&self, level: u8) -> f64 {
        self.size * self.scale(level)
    }
}

impl Default for LayoutStyle {
    /// Ten point at full size, with the customary 0.7 and 0.5 nesting factors.
    fn default() -> Self {
        Self { size: 10.0, script_scale: 0.7, script_script_scale: 0.5, cramped: false, level: 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::LayoutStyle;

    #[test]
    fn scale_saturates_at_the_second_factor() {
        let style = LayoutStyle::display(12.0);
        assert_eq!(style.scale(0), 1.0);
        assert_eq!(style.scale(1), 0.7);
        assert_eq!(style.scale(2), 0.5);
        assert_eq!(style.scale(9), 0.5);
        assert_eq!(style.size_at(0), 12.0);
        assert_eq!(style.size_at(1), 12.0 * 0.7);
    }

    #[test]
    fn cramped_and_level_survive_the_struct_update_syntax() {
        let style = LayoutStyle { cramped: true, level: 2, ..LayoutStyle::display(8.0) };
        assert!(style.cramped);
        assert_eq!(style.level, 2);
        assert_eq!(style.size, 8.0);
        assert_eq!(style.script_scale, 0.7);
    }
}
