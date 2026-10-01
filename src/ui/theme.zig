//! Approximate POC card colours. Graphical chrome uses these; never box glyphs.

const gtk = @import("gtk");

pub const BACKGROUND = .{ 0x11, 0x13, 0x18 };
pub const CARD = .{ 0x17, 0x1a, 0x20 };
pub const BORDER_ALPHA: f32 = 0.08;
pub const RADIUS_PX: f32 = 8.0;
pub const SHADOW_OFFSET_Y: f32 = 4.0;
pub const SHADOW_BLUR: f32 = 18.0;
pub const SHADOW_ALPHA: f32 = 0.30;
pub const PADDING_PX: f32 = 12.0;

pub const CSS_BACKGROUND_CLASS: [:0]const u8 = "herdr-shell-background";
pub const CSS_CARD_CLASS: [:0]const u8 = "herdr-card";

/// CSS for the shell background and the shared card chrome.
pub const stylesheet: [:0]const u8 =
    \\.herdr-shell-background {
    \\  background-color: #111318;
    \\}
    \\.herdr-card {
    \\  border-radius: 12px;
    \\  background-color: #171a20;
    \\  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.28), 0 1px 2px rgba(0, 0, 0, 0.18);
    \\}
    \\.herdr-card-header {
    \\  padding: 8px 12px 4px 12px;
    \\  font-weight: 600;
    \\}
    \\.herdr-card scrolledwindow,
    \\.herdr-card viewport,
    \\.herdr-card overlay,
    \\.herdr-card list,
    \\.herdr-card textview,
    \\.herdr-card text {
    \\  background-color: transparent;
    \\}
    \\list.herdr-rows > row,
    \\list.herdr-rows > row:selected {
    \\  background-color: transparent;
    \\  box-shadow: none;
    \\}
    \\row.herdr-row {
    \\  border-radius: 8px;
    \\  margin: 0 8px;
    \\  padding: 1px 8px;
    \\  background-color: transparent;
    \\}
    \\list.herdr-rows > row.herdr-row.herdr-selected,
    \\list.herdr-rows > row.herdr-row.herdr-selected:selected {
    \\  background-color: alpha(@accent_bg_color, 0.28);
    \\}
    \\row.herdr-row.herdr-nav {
    \\  margin: 2px 6px;
    \\  padding: 6px 8px;
    \\}
    \\list.herdr-rows > row.herdr-nav:hover:not(.herdr-selected) {
    \\  background-color: alpha(@theme_fg_color, 0.07);
    \\}
    \\list.herdr-rows > row.herdr-nav.herdr-selected,
    \\list.herdr-rows > row.herdr-nav.herdr-selected:hover,
    \\list.herdr-rows > row.herdr-nav.herdr-selected:selected {
    \\  background-color: alpha(@theme_fg_color, 0.08);
    \\  box-shadow: none;
    \\}
    \\paned.herdr-split > separator {
    \\  background-color: #111318;
    \\  background-image: none;
    \\  border: none;
    \\  border-right: 1px solid rgba(255, 255, 255, 0.08);
    \\  min-width: 8px;
    \\}
    \\paned.herdr-split > separator:hover {
    \\  background-color: #171a20;
    \\  border-right-color: rgba(255, 255, 255, 0.14);
    \\}
    \\.herdr-meta {
    \\  opacity: 0.65;
    \\  font-size: 0.86em;
    \\}
    \\.herdr-status {
    \\  min-width: 8px;
    \\  min-height: 8px;
    \\  border-radius: 8px;
    \\}
    \\.herdr-status-idle {
    \\  background-color: #8b929e;
    \\}
    \\.herdr-status-working {
    \\  background-color: #e6b84d;
    \\}
    \\.herdr-status-blocked {
    \\  background-color: #e07050;
    \\}
    \\.herdr-status-done {
    \\  background-color: #6bbf8a;
    \\}
    \\.herdr-status-unknown {
    \\  background-color: #5a606c;
    \\}
    \\scrolledwindow.herdr-scroll scrollbar,
    \\scrollbar.herdr-scroll {
    \\  min-width: 8px;
    \\}
    \\scrolledwindow.herdr-scroll scrollbar trough,
    \\scrollbar.herdr-scroll trough {
    \\  min-width: 8px;
    \\  background-color: alpha(@theme_fg_color, 0.14);
    \\  border-radius: 8px;
    \\}
    \\scrolledwindow.herdr-scroll scrollbar slider,
    \\scrollbar.herdr-scroll slider {
    \\  min-width: 8px;
    \\  min-height: 24px;
    \\  background-color: alpha(@theme_fg_color, 0.55);
    \\  border-radius: 8px;
    \\}
;

const Display = @typeInfo(@TypeOf(gtk.Widget.getDisplay)).@"fn".return_type.?;

pub fn installForDisplay(display: Display) void {
    const provider = gtk.CssProvider.new();
    provider.loadFromString(stylesheet);
    gtk.StyleContext.addProviderForDisplay(
        display,
        provider.as(gtk.StyleProvider),
        gtk.STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
