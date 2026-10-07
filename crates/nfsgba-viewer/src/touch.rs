//! The on-screen pad for touch screens: the GBA's buttons drawn over the game (Bevy UI) and the keys they hold, from
//! every finger on the screen each frame (several at once; a thumb sliding over the D-pad changes direction). One
//! table ([`BUTTONS`]) places the buttons and finds what a finger touches. Shown on Android from the start and on
//! other platforms after the first touch; the web page has its own pad (`web/index.html`), so it stays off there.

use bevy::{input::touch::Touches, prelude::*};

/// The keys the pad holds this frame (the GBA's key bits: A 0, B 1, SELECT 2, START 3, RIGHT 4, LEFT 5, UP 6,
/// DOWN 7, R 8, L 9), and whether it is shown.
#[derive(Resource, Default)]
pub struct TouchPad {
    pub held: u16,
    shown: bool,
}

/// A button: its key bits, its label and its place as shares of the window (left, top, width, height).
struct Button {
    keys: u16,
    label: &'static str,
    rect: [f32; 4],
}

/// The D-pad: one square, its direction from the centre (the eight ways).
const DPAD: [f32; 4] = [0.02, 0.42, 0.22, 0.33];
/// Inside this share of the D-pad's half-size around its centre no direction is held.
const DEAD: f32 = 0.25;
const BUTTONS: [Button; 6] = [
    Button {
        keys: 1,
        label: "A",
        rect: [0.86, 0.50, 0.12, 0.18],
    },
    Button {
        keys: 2,
        label: "B",
        rect: [0.73, 0.64, 0.12, 0.18],
    },
    Button {
        keys: 1 << 9,
        label: "L",
        rect: [0.0, 0.04, 0.14, 0.12],
    },
    Button {
        keys: 1 << 8,
        label: "R",
        rect: [0.86, 0.04, 0.14, 0.12],
    },
    Button {
        keys: 1 << 2,
        label: "SELECT",
        rect: [0.34, 0.88, 0.14, 0.09],
    },
    Button {
        keys: 1 << 3,
        label: "START",
        rect: [0.52, 0.88, 0.14, 0.09],
    },
];

fn inside(rect: [f32; 4], p: Vec2) -> bool {
    let [x, y, w, h] = rect;
    p.x >= x && p.x < x + w && p.y >= y && p.y < y + h
}

/// The keys held by fingers at `points` (shares of the window, from the top left).
pub fn keys_at(points: impl IntoIterator<Item = Vec2>) -> u16 {
    let mut keys = 0;
    for p in points {
        for b in &BUTTONS {
            if inside(b.rect, p) {
                keys |= b.keys;
            }
        }
        if inside(DPAD, p) {
            let [x, y, w, h] = DPAD;
            // The finger's place in the pad, -1..1 each way (y down).
            let d = Vec2::new((p.x - x) / w * 2.0 - 1.0, (p.y - y) / h * 2.0 - 1.0);
            keys |= match () {
                _ if d.x > DEAD => 1 << 4,
                _ if d.x < -DEAD => 1 << 5,
                _ => 0,
            };
            keys |= match () {
                _ if d.y < -DEAD => 1 << 6,
                _ if d.y > DEAD => 1 << 7,
                _ => 0,
            };
        }
    }
    keys
}

/// The pad's root node, shown or hidden as a whole.
#[derive(Component)]
struct Pad;

pub fn plugin(app: &mut App) {
    app.init_resource::<TouchPad>()
        .init_resource::<PadButtons>()
        .add_systems(PreUpdate, (read_touches, read_pad_buttons));
}

/// Spawns the pad (hidden until [`TouchPad`] says it is shown) on the UI camera `camera`.
pub fn spawn(commands: &mut Commands, camera: bevy::ui::UiTargetCamera) {
    let pct = |v: f32| Val::Percent(100.0 * v);
    let place = |[x, y, w, h]: [f32; 4]| Node {
        position_type: PositionType::Absolute,
        left: pct(x),
        top: pct(y),
        width: pct(w),
        height: pct(h),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border: UiRect::all(Val::Px(2.0)),
        ..default()
    };
    let colours = (
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12)),
        BorderColor::all(Color::srgba(1.0, 1.0, 1.0, 0.45)),
    );
    let text = |label: &str| {
        (
            Text::new(label),
            TextFont::from_font_size(22.0),
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
        )
    };
    let shown = cfg!(target_os = "android");
    commands
        .spawn((
            Pad,
            camera,
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            if shown {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            },
            GlobalZIndex(10),
        ))
        .with_children(|pad| {
            pad.spawn((place(DPAD), colours)).with_children(|d| {
                d.spawn(text("+"));
            });
            for b in &BUTTONS {
                pad.spawn((place(b.rect), colours)).with_children(|n| {
                    n.spawn(text(b.label));
                });
            }
        });
    commands.insert_resource(TouchPad { held: 0, shown });
}

/// The keys of every finger on the screen; the first touch shows the pad (not on the web: the page's pad is there).
fn read_touches(
    touches: Res<Touches>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    mut pad: ResMut<TouchPad>,
    mut root: Query<&mut Visibility, With<Pad>>,
) {
    let Some(window) = window else { return };
    let size = window.size();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let mut any = false;
    let held = keys_at(touches.iter().map(|t| {
        any = true;
        t.position() / size
    }));
    if any && !pad.shown && !cfg!(target_arch = "wasm32") {
        pad.shown = true;
        for mut v in &mut root {
            *v = Visibility::Inherited;
        }
    }
    pad.held = if pad.shown { held } else { 0 };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn centre(r: [f32; 4]) -> Vec2 {
        Vec2::new(r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)
    }

    /// Each button holds its key, the D-pad its direction (diagonals too, nothing in the middle), several fingers
    /// add up, and nothing outside the buttons holds a key.
    #[test]
    fn fingers_hold_the_keys_they_touch() {
        for b in &BUTTONS {
            assert_eq!(keys_at([centre(b.rect)]), b.keys, "{}", b.label);
        }
        let [x, y, w, h] = DPAD;
        let at = |fx: f32, fy: f32| keys_at([Vec2::new(x + w * fx, y + h * fy)]);
        assert_eq!(at(0.5, 0.5), 0, "the centre");
        assert_eq!(at(0.95, 0.5), 1 << 4, "right");
        assert_eq!(at(0.05, 0.5), 1 << 5, "left");
        assert_eq!(at(0.5, 0.05), 1 << 6, "up");
        assert_eq!(at(0.5, 0.95), 1 << 7, "down");
        assert_eq!(at(0.95, 0.05), 1 << 4 | 1 << 6, "up-right");
        // The A button and LEFT at once (accelerate into a turn).
        let a = centre(BUTTONS[0].rect);
        assert_eq!(keys_at([a, Vec2::new(x + w * 0.05, y + h * 0.5)]), 1 | 1 << 5);
        assert_eq!(keys_at([Vec2::new(0.5, 0.4)]), 0, "the middle of the screen");
        // No two buttons overlap, none overlaps the D-pad.
        let all: Vec<[f32; 4]> = BUTTONS.iter().map(|b| b.rect).chain([DPAD]).collect();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                let apart = a[0] + a[2] <= b[0] || b[0] + b[2] <= a[0] || a[1] + a[3] <= b[1] || b[1] + b[3] <= a[1];
                assert!(apart, "{a:?} and {b:?} overlap");
            }
        }
    }
}

/// Gamepad buttons that reach the game as key events with their Android key codes (winit passes them on unmapped:
/// `KeyCode::Unidentified(NativeKeyCode::Android(code))`; gilrs has no Android backend): BUTTON_A/Y = A,
/// BUTTON_B/X = B, L1/L2 = L, R1/R2 = R, START, SELECT. A controller's D-pad arrives as the arrow keys.
pub fn android_button(code: u32) -> u16 {
    match code {
        96 | 100 => 1,       // BUTTON_A, BUTTON_Y
        97 | 99 => 2,        // BUTTON_B, BUTTON_X
        109 => 1 << 2,       // BUTTON_SELECT
        108 => 1 << 3,       // BUTTON_START
        103 | 105 => 1 << 8, // BUTTON_R1, BUTTON_R2
        102 | 104 => 1 << 9, // BUTTON_L1, BUTTON_L2
        _ => 0,
    }
}

/// The game keys held on a controller whose buttons come as Android key codes ([`android_button`]): held now, and
/// pressed since the last display frame (a press and release within one frame still counts for that frame).
#[derive(Resource, Default)]
pub struct PadButtons {
    held: u16,
    pressed: u16,
}

impl PadButtons {
    pub fn keys(&self) -> u16 {
        self.held | self.pressed
    }
}

fn read_pad_buttons(mut events: MessageReader<bevy::input::keyboard::KeyboardInput>, mut b: ResMut<PadButtons>) {
    use bevy::input::keyboard::NativeKeyCode;
    b.pressed = 0;
    for e in events.read() {
        if let KeyCode::Unidentified(NativeKeyCode::Android(code)) = e.key_code {
            let bits = android_button(code);
            if bits == 0 {
                debug!("an unmapped Android key code: {code}");
            } else if e.state.is_pressed() {
                b.held |= bits;
                b.pressed |= bits;
            } else {
                b.held &= !bits;
            }
        }
    }
}

#[cfg(test)]
mod pad_tests {
    use super::*;

    #[test]
    fn android_buttons_are_the_gba_keys() {
        assert_eq!(android_button(96), 1);
        assert_eq!(android_button(97), 2);
        assert_eq!(android_button(108), 8);
        assert_eq!(android_button(109), 4);
        assert_eq!(android_button(102) | android_button(104), 1 << 9);
        assert_eq!(android_button(103) | android_button(105), 1 << 8);
        assert_eq!(
            android_button(29),
            0,
            "KEYCODE_A (a keyboard letter) is not a gamepad button"
        );
    }
}
