//! Approximate POC card colours. Graphical chrome uses these; never box glyphs.

pub struct Theme;

impl Theme {
    pub const BACKGROUND: (u8, u8, u8) = (0x11, 0x13, 0x18);
    pub const CARD: (u8, u8, u8) = (0x17, 0x1a, 0x20);
    pub const BORDER_ALPHA: f32 = 0.08;
    pub const RADIUS_PX: f32 = 8.0;
    pub const SHADOW_OFFSET_Y: f32 = 4.0;
    pub const SHADOW_BLUR: f32 = 18.0;
    pub const SHADOW_ALPHA: f32 = 0.30;
    pub const PADDING_PX: f32 = 12.0;

    pub const CSS_BACKGROUND_CLASS: &str = "herdr-shell-background";
    pub const CSS_CARD_CLASS: &str = "herdr-card";

    /// CSS for the shell background and the shared card chrome.
    ///
    /// Overlay focus rings stay in the pane stylesheet; they depend on the
    /// live border color. GTK maps radius and shadow through GSK.
    pub fn stylesheet() -> String {
        let (br, bg, bb) = Self::BACKGROUND;
        let (cr, cg, cb) = Self::CARD;
        let border_alpha = Self::BORDER_ALPHA;
        let border_hover = (Self::BORDER_ALPHA * 1.75).clamp(0.0, 1.0);
        format!(
            "\
.{bg_class} {{\n\
  background-color: #{br:02x}{bg:02x}{bb:02x};\n\
}}\n\
.herdr-card {{\n\
  border-radius: 12px;\n\
  background-color: #{cr:02x}{cg:02x}{cb:02x};\n\
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.28), 0 1px 2px rgba(0, 0, 0, 0.18);\n\
}}\n\
.herdr-card-header {{\n\
  padding: 8px 12px 4px 12px;\n\
  font-weight: 600;\n\
}}\n\
.herdr-card scrolledwindow,\n\
.herdr-card viewport,\n\
.herdr-card overlay,\n\
.herdr-card list,\n\
.herdr-card textview,\n\
.herdr-card text {{\n\
  background-color: transparent;\n\
}}\n\
list.herdr-rows > row,\n\
list.herdr-rows > row:selected {{\n\
  background-color: transparent;\n\
  box-shadow: none;\n\
}}\n\
row.herdr-row {{\n\
  border-radius: 8px;\n\
  margin: 0 8px;\n\
  padding: 1px 8px;\n\
  background-color: transparent;\n\
}}\n\
list.herdr-rows > row.herdr-row.herdr-selected,\n\
list.herdr-rows > row.herdr-row.herdr-selected:selected {{\n\
  background-color: alpha(@accent_bg_color, 0.28);\n\
}}\n\
row.herdr-row.herdr-nav {{\n\
  margin: 2px 6px;\n\
  padding: 6px 8px;\n\
}}\n\
list.herdr-rows > row.herdr-nav:hover:not(.herdr-selected) {{\n\
  background-color: alpha(@theme_fg_color, 0.07);\n\
}}\n\
list.herdr-rows > row.herdr-nav.herdr-selected,\n\
list.herdr-rows > row.herdr-nav.herdr-selected:hover,\n\
list.herdr-rows > row.herdr-nav.herdr-selected:selected {{\n\
  background-color: alpha(@theme_fg_color, 0.08);\n\
  box-shadow: none;\n\
}}\n\
paned.herdr-split > separator {{\n\
  background-color: #{br:02x}{bg:02x}{bb:02x};\n\
  background-image: none;\n\
  border: none;\n\
  border-right: 1px solid rgba(255, 255, 255, {border_alpha});\n\
  min-width: 8px;\n\
}}\n\
paned.herdr-split > separator:hover {{\n\
  background-color: #{cr:02x}{cg:02x}{cb:02x};\n\
  border-right-color: rgba(255, 255, 255, {border_hover});\n\
}}\n\
.herdr-meta {{\n\
  opacity: 0.65;\n\
  font-size: 0.86em;\n\
}}\n\
.herdr-status {{\n\
  min-width: 8px;\n\
  min-height: 8px;\n\
  border-radius: 8px;\n\
}}\n\
.herdr-status-idle {{\n\
  background-color: #8b929e;\n\
}}\n\
.herdr-status-working {{\n\
  background-color: #e6b84d;\n\
}}\n\
.herdr-status-blocked {{\n\
  background-color: #e07050;\n\
}}\n\
.herdr-status-done {{\n\
  background-color: #6bbf8a;\n\
}}\n\
.herdr-status-unknown {{\n\
  background-color: #5a606c;\n\
}}\n\
scrolledwindow.herdr-scroll scrollbar,\n\
scrollbar.herdr-scroll {{\n\
  min-width: 8px;\n\
}}\n\
scrolledwindow.herdr-scroll scrollbar trough,\n\
scrollbar.herdr-scroll trough {{\n\
  min-width: 8px;\n\
  background-color: alpha(@theme_fg_color, 0.14);\n\
  border-radius: 8px;\n\
}}\n\
scrolledwindow.herdr-scroll scrollbar slider,\n\
scrollbar.herdr-scroll slider {{\n\
  min-width: 8px;\n\
  min-height: 24px;\n\
  background-color: alpha(@theme_fg_color, 0.55);\n\
  border-radius: 8px;\n\
}}\n",
            bg_class = Self::CSS_BACKGROUND_CLASS,
            br = br,
            bg = bg,
            bb = bb,
            cr = cr,
            cg = cg,
            cb = cb,
            border_alpha = border_alpha,
            border_hover = border_hover,
        )
    }

    #[cfg(feature = "gtk")]
    pub fn install_for_display(display: &gtk::gdk::Display) {
        let provider = gtk::CssProvider::new();
        provider.load_from_string(&Self::stylesheet());
        gtk::style_context_add_provider_for_display(
            display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::Theme;

    #[test]
    fn stylesheet_uses_theme_constants() {
        let css = Theme::stylesheet();
        assert!(css.contains("#111318"));
        assert!(css.contains("#171a20"));
        assert!(css.contains("border-radius: 12px"));
        assert!(css.contains("box-shadow: 0 8px 24px"));
        assert!(css.contains("alpha(@accent_bg_color, 0.28)"));
        assert!(css.contains("list.herdr-rows > row.herdr-nav.herdr-selected"));
        assert!(css.contains("alpha(@theme_fg_color, 0.08)"));
        assert!(css.contains("paned.herdr-split > separator"));
        assert!(css.contains("border-right: 1px solid rgba(255, 255, 255, 0.08)"));
        assert!(css.contains("paned.herdr-split > separator:hover"));
        assert!(!css.contains("border-left: 1px solid alpha(@theme_fg_color"));
        assert!(!css.contains("inset 3px 0 0 @accent_color"));
        assert!(css.contains(".herdr-status-working"));
        assert!(css.contains("#e6b84d"));
        assert!(css.contains(".herdr-status-blocked"));
        assert!(css.contains("#e07050"));
        assert!(css.contains(".herdr-status-done"));
        assert!(css.contains("#6bbf8a"));
        assert!(css.contains(".herdr-status-idle"));
        assert!(css.contains(".herdr-status-unknown"));
        assert!(css.contains("scrolledwindow.herdr-scroll scrollbar"));
        assert!(!css.contains("padding: 12"));
        assert!(!css.contains('┌'));
        assert!(!css.contains('─'));
    }
}
