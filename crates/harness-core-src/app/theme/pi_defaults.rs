use crate::render::Color;
// Defaults transcribed from Pi's built-in theme (dark.json); session uplink overrides these.
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
