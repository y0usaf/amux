use crate::render::Color;
// Defaults transcribed from Pi's built-in themes (packages/coding-agent/src/modes/interactive/theme/{dark,light}.json); session uplink overrides these.
pub(super) fn dark() -> [Color; 15] {
    [
        c("#d4d4d4"),
        c("#808080"),
        c("#f0c674"),
        c("#8abeb7"),
        c("#00d7ff"),
        c("#505050"),
        c("#282832"),
        c("#3a3a4a"),
        Color::rgba(0, 0, 0, 0),
        c("#8abeb7"),
        c("#283228"),
        c("#81a2be"),
        c("#b5bd68"),
        c("#ffff00"),
        c("#cc6666"),
    ]
}
pub(super) fn light() -> [Color; 15] {
    [
        c("#1f2328"),
        c("#6c6c6c"),
        c("#9a7326"),
        c("#5a8080"),
        c("#5a8080"),
        c("#b0b0b0"),
        c("#e8e8f0"),
        c("#d0d0e0"),
        Color::rgba(0, 0, 0, 0),
        c("#5a8080"),
        c("#e8f0e8"),
        c("#547da7"),
        c("#588458"),
        c("#9a7326"),
        c("#aa5555"),
    ]
}
const fn c(s: &str) -> Color {
    let b = s.as_bytes();
    const fn h(x: u8) -> u8 {
        match x {
            b'0'..=b'9' => x - b'0',
            b'a'..=b'f' => x - b'a' + 10,
            _ => 0,
        }
    }
    Color::rgb(
        h(b[1]) * 16 + h(b[2]),
        h(b[3]) * 16 + h(b[4]),
        h(b[5]) * 16 + h(b[6]),
    )
}
