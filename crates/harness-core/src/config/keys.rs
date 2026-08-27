use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyStroke {
    pub modifiers: KeyModifiers,
    pub key: KeyToken,
}

impl KeyStroke {
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

        Ok(Self {
            modifiers,
            key: KeyToken::parse(key)?,
        })
    }
}

impl fmt::Display for KeyStroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::with_capacity(4);
        if self.modifiers.control {
            parts.push("ctrl".to_string());
        }
        if self.modifiers.shift {
            parts.push("shift".to_string());
        }
        if self.modifiers.alt {
            parts.push("alt".to_string());
        }
        parts.push(self.key.to_string());
        write!(f, "{}", parts.join("+"))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyModifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
}

impl KeyModifiers {
    fn apply_token(&mut self, token: &str) -> Result<(), String> {
        let normalized = token.trim().to_ascii_lowercase();
        let enabled = match normalized.as_str() {
            "ctrl" | "control" => &mut self.control,
            "shift" => &mut self.shift,
            "alt" | "option" => &mut self.alt,
            other => return Err(format!("unknown modifier: {other}")),
        };
        if *enabled {
            return Err(format!("duplicate modifier: {normalized}"));
        }
        *enabled = true;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyToken {
    Character(String),
    Named(NamedKeyToken),
}

impl KeyToken {
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

    #[test]
    fn parse_normalizes_modifiers_aliases_and_named_keys() {
        let stroke = KeyStroke::parse("  Control + Option + Page_Down ").unwrap();

        assert_eq!(
            stroke,
            KeyStroke {
                modifiers: KeyModifiers {
                    control: true,
                    shift: false,
                    alt: true,
                },
                key: KeyToken::Named(NamedKeyToken::PageDown),
            }
        );
        assert_eq!(stroke.to_string(), "ctrl+alt+pagedown");
    }

    #[test]
    fn parse_supports_special_character_aliases_and_canonical_display() {
        let plus = KeyStroke::parse("ctrl+plus").unwrap();
        let minus = KeyStroke::parse("SHIFT + minus").unwrap();

        assert_eq!(plus.key, KeyToken::Character("+".to_string()));
        assert_eq!(minus.key, KeyToken::Character("-".to_string()));

        assert_eq!(plus.to_string(), "ctrl+plus");
        assert_eq!(minus.to_string(), "shift+minus");
    }

    #[test]
    fn parse_lowercases_character_keys_and_preserves_modifier_ordering_in_display() {
        let stroke = KeyStroke::parse("SHIFT+Alt+Control+X").unwrap();

        assert_eq!(stroke.to_string(), "ctrl+shift+alt+x");
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
    fn backspace_aliases_are_supported() {
        let parsed = KeyStroke::parse("shift+back").unwrap();
        assert_eq!(parsed.to_string(), "shift+backspace");
    }
}
