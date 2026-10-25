//! Run-time colour overrides from the embedding host (C0VIBE).
//!
//! Not upstream's. The host sends `{tokenName: "#rrggbb" | "#rrggbbaa"}` (the two `[Color32; 2]`
//! tokens take an array of two colours); the names are the fields of [`Tokens`]. The overrides
//! lie over both presets, so they stay in force when the person switches Light and Dark.
//! [`resolve`] is the one place [`Tokens::current`] reads them, so every frame sees them.
//!
//! A call merges into what is already set: a `null` value takes one override away, `null` for the
//! whole set takes all of them away.

use super::Tokens;
use egui::Color32;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// One override: a single colour or the pair of a two-colour token.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Colour {
    One(Color32),
    Two([Color32; 2]),
}

enum Slot<'a> {
    One(&'a mut Color32),
    Two(&'a mut [Color32; 2]),
}

macro_rules! token_table {
    (one: $($one:ident),+ ; two: $($two:ident),+ $(,)?) => {
        /// Every token the host may set: the fields of [`Tokens`], in declaration order.
        pub const TOKEN_NAMES: &[&str] = &[$(stringify!($one),)+ $(stringify!($two),)+];

        fn slot<'a>(tokens: &'a mut Tokens, name: &str) -> Option<Slot<'a>> {
            $(if name == stringify!($one) { return Some(Slot::One(&mut tokens.$one)); })+
            $(if name == stringify!($two) { return Some(Slot::Two(&mut tokens.$two)); })+
            None
        }
    };
}

token_table! {
    one: window_bg, panel_bg, panel_bg2, toolbar_bg, toolbar_group, border, border_light, edge_light, text, text_dim, text_dark,
        header_text, counter_bg, counter_text, counter_label, accent, accent_dark, mode_on, mode_on_text, mode_off_text, button,
        button_hi, button_border, name_field, name_field_sel, ruler_bg, ruler_label_bg, ruler_text, ruler_tick, tempo_ruler,
        meter_ruler, marker_ruler, playlist_bg, playlist_alt, grid_line, bar_line, selection, playhead, insertion, solo, mute,
        rec, input, auto_read, auto_write, auto_touch, auto_latch, meter_green, meter_yellow, meter_red, fader_track, strip_bg,
        strip_section, slot_bg, automation_line, menu_bar, group_border, mode_off, counter_off, counter_border, counter_rule,
        tool_bracket_off, row_selected, visible_dot, big_counter_bg, header_selected, strip_selected, marker_text, playlist_lane,
        universe_bg, universe_view, meter_bg, meter_clip_off, knob_bg, knob_ring, knob_pointer, section_label, slot_well,
        slot_empty, slot_mark;
    two: slot_face, slot_bypassed
}

/// The overrides and the two presets with them applied (dark, light).
struct Resolved {
    set: BTreeMap<&'static str, Colour>,
    presets: [Tokens; 2],
}

static STATE: RwLock<Option<Resolved>> = RwLock::new(None);
/// Whether any override is set: the fast path of [`resolve`], which runs for every colour read.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// What a call did, per token name.
#[derive(Debug, Default, PartialEq)]
pub struct Report {
    pub applied: Vec<String>,
    pub unknown: Vec<String>,
    pub invalid: Vec<String>,
}

impl Report {
    pub fn to_json(&self) -> Value {
        json!({"applied": self.applied, "unknown": self.unknown, "invalid": self.invalid})
    }
}

/// `#rrggbb` or `#rrggbbaa` (the `#` is optional); the alpha is not premultiplied.
pub fn parse_color(text: &str) -> Option<Color32> {
    let hex = text.trim().trim_start_matches('#');
    if !hex.is_ascii() || !(hex.len() == 6 || hex.len() == 8) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    let (r, g, b) = (byte(0)?, byte(2)?, byte(4)?);
    let a = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Color32::from_rgba_unmultiplied(r, g, b, a))
}

fn parse_value(value: &Value, pair: bool) -> Option<Colour> {
    if pair {
        let items = value.as_array().filter(|a| a.len() == 2)?;
        Some(Colour::Two([parse_color(items[0].as_str()?)?, parse_color(items[1].as_str()?)?]))
    } else {
        Some(Colour::One(parse_color(value.as_str()?)?))
    }
}

fn is_pair(name: &str) -> bool {
    matches!(name, "slot_face" | "slot_bypassed")
}

fn lock_read() -> std::sync::RwLockReadGuard<'static, Option<Resolved>> {
    STATE.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn lock_write() -> std::sync::RwLockWriteGuard<'static, Option<Resolved>> {
    STATE.write().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn build(set: BTreeMap<&'static str, Colour>) -> Option<Resolved> {
    if set.is_empty() {
        return None;
    }
    let mut presets = [Tokens::DARK, Tokens::LIGHT];
    for preset in &mut presets {
        for (name, colour) in &set {
            match (slot(preset, name), colour) {
                (Some(Slot::One(to)), Colour::One(c)) => *to = *c,
                (Some(Slot::Two(to)), Colour::Two(c)) => *to = *c,
                _ => {}
            }
        }
    }
    Some(Resolved { set, presets })
}

/// The tokens of the active preset, with the host's colours over them. [`Tokens::current`] calls this.
pub fn resolve(light: bool) -> Tokens {
    let base = if light { Tokens::LIGHT } else { Tokens::DARK };
    if !ACTIVE.load(Ordering::Relaxed) {
        return base;
    }
    lock_read().as_ref().map_or(base, |r| r.presets[usize::from(light)])
}

/// The names with an override at the moment.
pub fn active() -> Vec<String> {
    lock_read().as_ref().map(|r| r.set.keys().map(|k| (*k).to_owned()).collect()).unwrap_or_default()
}

/// Takes every override away.
pub fn clear() {
    *lock_write() = None;
    ACTIVE.store(false, Ordering::Relaxed);
}

/// Merges `tokens` (an object of name to colour; `null` values take one away, `null` all of them)
/// into the overrides. Nothing is changed for a name that is unknown or invalid.
pub fn set(tokens: &Value) -> Result<Report, String> {
    if tokens.is_null() {
        clear();
        return Ok(Report::default());
    }
    let fields = tokens.as_object().ok_or("`themeTokens` must be an object of token name to colour")?;
    let mut report = Report::default();
    let mut guard = lock_write();
    let mut next: BTreeMap<&'static str, Colour> = guard.as_ref().map(|r| r.set.clone()).unwrap_or_default();
    for (name, value) in fields {
        let Some(&known) = TOKEN_NAMES.iter().find(|n| **n == name.as_str()) else {
            report.unknown.push(name.clone());
            continue;
        };
        if value.is_null() {
            next.remove(known);
            report.applied.push(name.clone());
        } else if let Some(colour) = parse_value(value, is_pair(known)) {
            next.insert(known, colour);
            report.applied.push(name.clone());
        } else {
            report.invalid.push(name.clone());
        }
    }
    *guard = build(next);
    ACTIVE.store(guard.is_some(), Ordering::Relaxed);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_parse_with_and_without_alpha() {
        assert_eq!(parse_color("#ff8000"), Some(Color32::from_rgb(255, 128, 0)));
        assert_eq!(parse_color("ff800080"), Some(Color32::from_rgba_unmultiplied(255, 128, 0, 128)));
        for bad in ["", "#fff", "#gg0000", "#ff80000", "#ff80", "red", "#ñ00000", "#ff8000 00"] {
            assert_eq!(parse_color(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn pairs_need_two_good_colours() {
        assert!(parse_value(&json!(["#000000", "#ffffff"]), true).is_some());
        assert!(parse_value(&json!(["#000000"]), true).is_none());
        assert!(parse_value(&json!("#000000"), true).is_none());
        assert!(parse_value(&json!(["#000000", 7]), true).is_none());
        assert!(parse_value(&json!(["#000000", "#ffffff"]), false).is_none());
    }

    #[test]
    fn the_table_names_every_token_once() {
        let mut names = TOKEN_NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), TOKEN_NAMES.len());
        let mut tokens = Tokens::DARK;
        for name in TOKEN_NAMES {
            assert!(slot(&mut tokens, name).is_some(), "{name}");
        }
    }
}
