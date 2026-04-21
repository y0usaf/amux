use std::fmt;

use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyStroke {
    pub modifiers: KeyModifiers,
    pub key: KeyToken,
}

impl KeyStroke {
    pub fn from_event(event: &KeyEvent, modifiers: ModifiersState) -> Option<Self> {
        if event.state != ElementState::Pressed {
            return None;
        }

        Some(Self {
            modifiers: modifiers.into(),
            key: KeyToken::from_event_key(&event.logical_key)?,
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

        Ok(Self {
            modifiers,
            key: KeyToken::parse(key)?,
        })
    }
}

impl fmt::Display for KeyStroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::with_capacity(5);
        if self.modifiers.control {
            parts.push("ctrl".to_string());
        }
        if self.modifiers.shift {
            parts.push("shift".to_string());
        }
        if self.modifiers.alt {
            parts.push("alt".to_string());
        }
        if self.modifiers.super_key {
            parts.push("cmd".to_string());
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
    pub super_key: bool,
}

impl KeyModifiers {
    fn apply_token(&mut self, token: &str) -> Result<(), String> {
        match token.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "control" => self.control = true,
            "shift" => self.shift = true,
            "alt" | "option" => self.alt = true,
            "cmd" | "command" | "super" | "meta" => self.super_key = true,
            other => return Err(format!("unknown modifier: {other}")),
        }
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
    fn from_event_key(key: &Key) -> Option<Self> {
        match key {
            Key::Character(text) => {
                if text.as_str() == " " {
                    return Some(Self::Named(NamedKeyToken::Space));
                }

                let text = text.as_str().trim().to_ascii_lowercase();
                if text.is_empty() {
                    None
                } else {
                    Some(Self::Character(text))
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
        };
        write!(f, "{text}")
    }
}
