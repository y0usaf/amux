use std::fmt;

use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyStroke {
    pub modifiers: KeyModifiers,
    pub key: KeyToken,
}

impl KeyStroke {
    pub fn from_event(event: &KeyEvent, modifiers: ModifiersState) -> Option<Self> {
        let key_without_modifiers = event.key_without_modifiers();
        Self::from_event_parts(
            event.state,
            event.repeat,
            &event.logical_key,
            Some(&key_without_modifiers),
            modifiers,
        )
    }

    fn from_event_parts(
        state: ElementState,
        repeat: bool,
        logical_key: &Key,
        key_without_modifiers: Option<&Key>,
        modifiers: ModifiersState,
    ) -> Option<Self> {
        if state != ElementState::Pressed || repeat {
            return None;
        }

        let key = key_without_modifiers.unwrap_or(logical_key);
        Some(Self {
            modifiers: modifiers.into(),
            key: KeyToken::from_event_key(key)?,
        })
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("empty key stroke".to_string());
        }

        let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
        if parts.iter().any(|part| part.is_empty()) {
            return Err(format!("invalid key stroke: {text}"));
        }
        let key = parts
            .pop()
            .ok_or_else(|| format!("invalid key stroke: {text}"))?;
        let mut modifiers = KeyModifiers::default();
        for modifier in parts {
            modifiers.apply_token(modifier)?;
        }

        let key = KeyToken::parse(key)?;
        let (modifiers, key) = normalize_shortcut_aliases(modifiers, key);

        Ok(Self { modifiers, key })
    }
}

fn normalize_shortcut_aliases(
    mut modifiers: KeyModifiers,
    mut key: KeyToken,
) -> (KeyModifiers, KeyToken) {
    let Some(base) = key.as_shifted_base_character() else {
        return (modifiers, key);
    };

    modifiers.shift = true;
    key = KeyToken::Character(base.to_string());
    (modifiers, key)
}

impl fmt::Display for KeyStroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::with_capacity(5);
        let collapse_shift_equal = self.modifiers.shift
            && matches!(self.key, KeyToken::Character(ref text) if text == "=");
        if self.modifiers.control {
            parts.push("ctrl".to_string());
        }
        if self.modifiers.shift && !collapse_shift_equal {
            parts.push("shift".to_string());
        }
        if self.modifiers.alt {
            parts.push("alt".to_string());
        }
        if self.modifiers.super_key {
            parts.push("cmd".to_string());
        }
        parts.push(if collapse_shift_equal {
            "plus".to_string()
        } else {
            self.key.to_string()
        });
        write!(f, "{}", parts.join("+"))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyModifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
}

impl KeyModifiers {
    fn apply_token(&mut self, token: &str) -> Result<(), String> {
        let normalized = token.trim().to_ascii_lowercase();
        let enabled = match normalized.as_str() {
            "ctrl" | "control" => &mut self.control,
            "shift" => &mut self.shift,
            "alt" | "option" => &mut self.alt,
            "cmd" | "command" | "super" | "meta" => &mut self.super_key,
            other => return Err(format!("unknown modifier: {other}")),
        };
        if *enabled {
            return Err(format!("duplicate modifier: {normalized}"));
        }
        *enabled = true;
        Ok(())
    }
}

impl From<ModifiersState> for KeyModifiers {
    fn from(modifiers: ModifiersState) -> Self {
        Self {
            control: modifiers.control_key(),
            shift: modifiers.shift_key(),
            alt: modifiers.alt_key(),
            super_key: modifiers.super_key(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyToken {
    Character(String),
    Named(NamedKeyToken),
}

impl KeyToken {
    fn as_shifted_base_character(&self) -> Option<char> {
        let Self::Character(text) = self else {
            return None;
        };

        let mut chars = text.chars();
        let ch = chars.next()?;
        if chars.next().is_some() {
            return None;
        }

        match ch {
            '!' => Some('1'),
            '@' => Some('2'),
            '#' => Some('3'),
            '$' => Some('4'),
            '%' => Some('5'),
            '^' => Some('6'),
            '&' => Some('7'),
            '*' => Some('8'),
            '(' => Some('9'),
            ')' => Some('0'),
            '_' => Some('-'),
            '+' => Some('='),
            '{' => Some('['),
            '}' => Some(']'),
            '|' => Some('\\'),
            ':' => Some(';'),
            '"' => Some('\''),
            '<' => Some(','),
            '>' => Some('.'),
            '?' => Some('/'),
            '~' => Some('`'),
            _ => None,
        }
    }

    fn from_event_key(key: &Key) -> Option<Self> {
        match key {
            Key::Character(text) => {
                if text.as_str() == " " {
                    return Some(Self::Named(NamedKeyToken::Space));
                }

                let text = text.as_str().trim().to_ascii_lowercase();
                if text.chars().count() == 1 {
                    Some(Self::Character(text))
                } else {
                    None
                }
            }
            Key::Named(NamedKey::ArrowLeft) => Some(Self::Named(NamedKeyToken::Left)),
            Key::Named(NamedKey::ArrowRight) => Some(Self::Named(NamedKeyToken::Right)),
            Key::Named(NamedKey::ArrowUp) => Some(Self::Named(NamedKeyToken::Up)),
            Key::Named(NamedKey::ArrowDown) => Some(Self::Named(NamedKeyToken::Down)),
            Key::Named(NamedKey::Delete) => Some(Self::Named(NamedKeyToken::Delete)),
            Key::Named(NamedKey::Insert) => Some(Self::Named(NamedKeyToken::Insert)),
            Key::Named(NamedKey::Home) => Some(Self::Named(NamedKeyToken::Home)),
            Key::Named(NamedKey::End) => Some(Self::Named(NamedKeyToken::End)),
            Key::Named(NamedKey::PageUp) => Some(Self::Named(NamedKeyToken::PageUp)),
            Key::Named(NamedKey::PageDown) => Some(Self::Named(NamedKeyToken::PageDown)),
            Key::Named(NamedKey::Enter) => Some(Self::Named(NamedKeyToken::Enter)),
            Key::Named(NamedKey::Tab) => Some(Self::Named(NamedKeyToken::Tab)),
            Key::Named(NamedKey::Escape) => Some(Self::Named(NamedKeyToken::Escape)),
            Key::Named(NamedKey::Space) => Some(Self::Named(NamedKeyToken::Space)),
            Key::Named(NamedKey::Backspace) => Some(Self::Named(NamedKeyToken::Backspace)),
            _ => None,
        }
    }

    fn parse(token: &str) -> Result<Self, String> {
        let token = token.trim().to_ascii_lowercase();
        if token.is_empty() {
            return Err("empty key".to_string());
        }

        let named = match token.as_str() {
            "left" | "arrowleft" => Some(NamedKeyToken::Left),
            "right" | "arrowright" => Some(NamedKeyToken::Right),
            "up" | "arrowup" => Some(NamedKeyToken::Up),
            "down" | "arrowdown" => Some(NamedKeyToken::Down),
            "delete" | "del" => Some(NamedKeyToken::Delete),
            "insert" | "ins" => Some(NamedKeyToken::Insert),
            "home" => Some(NamedKeyToken::Home),
            "end" => Some(NamedKeyToken::End),
            "pageup" | "page-up" | "page_up" => Some(NamedKeyToken::PageUp),
            "pagedown" | "page-down" | "page_down" => Some(NamedKeyToken::PageDown),
            "enter" | "return" => Some(NamedKeyToken::Enter),
            "tab" => Some(NamedKeyToken::Tab),
            "esc" | "escape" => Some(NamedKeyToken::Escape),
            "space" => Some(NamedKeyToken::Space),
            "backspace" | "back" | "bs" => Some(NamedKeyToken::Backspace),
            _ => None,
        };
        if let Some(named) = named {
            return Ok(Self::Named(named));
        }

        let character = match token.as_str() {
            "plus" => "+",
            "minus" => "-",
            "equal" | "equals" => "=",
            other => other,
        };
        if character.chars().count() == 1 {
            return Ok(Self::Character(character.to_string()));
        }

        Err(format!("unknown key: {token}"))
    }
}

impl fmt::Display for KeyToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Character(text) => match text.as_str() {
                "+" => write!(f, "plus"),
                "-" => write!(f, "minus"),
                "=" => write!(f, "equal"),
                _ => write!(f, "{text}"),
            },
            Self::Named(named) => write!(f, "{named}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NamedKeyToken {
    Left,
    Right,
    Up,
    Down,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Enter,
    Tab,
    Escape,
    Space,
    Backspace,
}

impl fmt::Display for NamedKeyToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
            Self::Delete => "delete",
            Self::Insert => "insert",
            Self::Home => "home",
            Self::End => "end",
            Self::PageUp => "pageup",
            Self::PageDown => "pagedown",
            Self::Enter => "enter",
            Self::Tab => "tab",
            Self::Escape => "escape",
            Self::Space => "space",
            Self::Backspace => "backspace",
        };
        write!(f, "{text}")
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyModifiers, KeyStroke, KeyToken, NamedKeyToken};
    use crate::config::{AppAction, AppConfig, KeyChordState, KeymapMatch};
    use winit::event::ElementState;
    use winit::keyboard::{Key, ModifiersState};

    fn stroke_from_keys(logical: Key, base: Key, modifiers: ModifiersState) -> KeyStroke {
        KeyStroke::from_event_parts(
            ElementState::Pressed,
            false,
            &logical,
            Some(&base),
            modifiers,
        )
        .unwrap()
    }

    #[test]
    fn parse_normalizes_modifiers_aliases_and_named_keys() {
        let stroke = KeyStroke::parse("  Control + Option + Meta + Page_Down ").unwrap();

        assert_eq!(
            stroke,
            KeyStroke {
                modifiers: KeyModifiers {
                    control: true,
                    shift: false,
                    alt: true,
                    super_key: true,
                },
                key: KeyToken::Named(NamedKeyToken::PageDown),
            }
        );
        assert_eq!(stroke.to_string(), "ctrl+alt+cmd+pagedown");
    }

    #[test]
    fn parse_supports_special_character_aliases_and_canonical_display() {
        let plus = KeyStroke::parse("ctrl+plus").unwrap();
        let shifted_equal = KeyStroke::parse("ctrl+shift+equal").unwrap();
        let minus = KeyStroke::parse("SHIFT + minus").unwrap();
        let equal = KeyStroke::parse("cmd+equals").unwrap();

        assert_eq!(plus, shifted_equal);
        assert_eq!(plus.key, KeyToken::Character("=".to_string()));
        assert_eq!(minus.key, KeyToken::Character("-".to_string()));
        assert_eq!(equal.key, KeyToken::Character("=".to_string()));

        assert_eq!(plus.to_string(), "ctrl+plus");
        assert_eq!(minus.to_string(), "shift+minus");
        assert_eq!(equal.to_string(), "cmd+equal");
    }

    #[test]
    fn parse_lowercases_character_keys_and_preserves_modifier_ordering_in_display() {
        let stroke = KeyStroke::parse("meta+SHIFT+Alt+Control+X").unwrap();

        assert_eq!(stroke.to_string(), "ctrl+shift+alt+cmd+x");
    }

    #[test]
    fn parse_rejects_empty_segments_unknown_modifier_and_unknown_key() {
        assert!(KeyStroke::parse("ctrl++").is_err());
        assert_eq!(
            KeyStroke::parse("hyper+x").unwrap_err(),
            "unknown modifier: hyper"
        );
        assert_eq!(
            KeyStroke::parse("ctrl+f13").unwrap_err(),
            "unknown key: f13"
        );
    }

    #[test]
    fn parse_rejects_duplicate_modifiers() {
        assert_eq!(
            KeyStroke::parse("ctrl+ctrl+x").unwrap_err(),
            "duplicate modifier: ctrl"
        );
        assert_eq!(
            KeyStroke::parse("alt+option+x").unwrap_err(),
            "duplicate modifier: option"
        );
        assert_eq!(
            KeyStroke::parse("meta+cmd+x").unwrap_err(),
            "duplicate modifier: cmd"
        );
    }

    #[test]
    fn parse_accepts_named_key_aliases() {
        assert_eq!(KeyStroke::parse("esc").unwrap().to_string(), "escape");
        assert_eq!(KeyStroke::parse("return").unwrap().to_string(), "enter");
        assert_eq!(KeyStroke::parse("page-up").unwrap().to_string(), "pageup");
        assert_eq!(KeyStroke::parse("ins").unwrap().to_string(), "insert");
        assert_eq!(KeyStroke::parse("bs").unwrap().to_string(), "backspace");
    }

    #[test]
    fn repeated_key_events_do_not_generate_shortcut_strokes() {
        assert!(KeyStroke::from_event_parts(
            ElementState::Pressed,
            true,
            &Key::Character("n".into()),
            Some(&Key::Character("n".into())),
            ModifiersState::CONTROL,
        )
        .is_none());
    }

    #[test]
    fn shifted_symbol_events_use_modifierless_base_key_for_matching() {
        let ctrl_plus = stroke_from_keys(
            Key::Character("+".into()),
            Key::Character("=".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );
        let shift_minus = stroke_from_keys(
            Key::Character("_".into()),
            Key::Character("-".into()),
            ModifiersState::SHIFT,
        );

        assert_eq!(ctrl_plus, KeyStroke::parse("ctrl+plus").unwrap());
        assert_eq!(ctrl_plus.to_string(), "ctrl+plus");
        assert_eq!(shift_minus, KeyStroke::parse("shift+minus").unwrap());
        assert_eq!(shift_minus.to_string(), "shift+minus");

        let keymap = AppConfig::default().keymap();
        let mut state = KeyChordState::default();
        assert_eq!(
            keymap.advance(&mut state, ctrl_plus),
            KeymapMatch::Triggered(AppAction::ZoomIn)
        );
    }

    #[test]
    fn shifted_letter_events_keep_shift_modifier() {
        let stroke = stroke_from_keys(
            Key::Character("R".into()),
            Key::Character("r".into()),
            ModifiersState::CONTROL | ModifiersState::SHIFT,
        );

        assert_eq!(stroke.to_string(), "ctrl+shift+r");

        let keymap = AppConfig::default().keymap();
        let mut state = KeyChordState::default();
        assert_eq!(
            keymap.advance(&mut state, stroke),
            KeymapMatch::Triggered(AppAction::RefreshAllSessions)
        );
    }

    #[test]
    fn backspace_aliases_and_events_are_supported() {
        let parsed = KeyStroke::parse("shift+back").unwrap();
        assert_eq!(parsed.to_string(), "shift+backspace");

        let stroke = stroke_from_keys(
            Key::Named(winit::keyboard::NamedKey::Backspace),
            Key::Named(winit::keyboard::NamedKey::Backspace),
            ModifiersState::SHIFT,
        );
        assert_eq!(stroke, parsed);
    }

    #[test]
    fn event_keys_reject_multi_character_text_and_accept_single_unicode_scalars() {
        assert_eq!(
            KeyToken::from_event_key(&Key::Character("é".into())),
            Some(KeyToken::Character("é".to_string()))
        );
        assert_eq!(KeyToken::from_event_key(&Key::Character("ab".into())), None);
        assert_eq!(
            KeyToken::from_event_key(&Key::Character("\r\n".into())),
            None
        );
    }

    #[test]
    fn shifted_character_literals_parse_like_base_key_plus_shift() {
        assert_eq!(
            KeyStroke::parse("!").unwrap(),
            KeyStroke::parse("shift+1").unwrap()
        );
        assert_eq!(
            KeyStroke::parse("?").unwrap(),
            KeyStroke::parse("shift+/").unwrap()
        );
    }
}
