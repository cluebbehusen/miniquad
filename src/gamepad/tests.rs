use super::*;

#[test]
fn product_categories_name_their_family() {
    let cases = [
        ("Xbox One", Family::Xbox),
        ("Xbox Series X", Family::Xbox),
        ("DualShock 4", Family::PlayStation),
        ("DualSense", Family::PlayStation),
        ("DualSense Edge", Family::PlayStation),
        ("Switch Pro Controller", Family::Switch),
        ("Nintendo Switch Joy-Con (L/R)", Family::Switch),
        ("MFi", Family::Other),
        ("HID", Family::Other),
        ("Arcade Stick", Family::Other),
    ];
    for (category, family) in cases {
        assert_eq!(
            Family::from_product_category(category),
            family,
            "{category}"
        );
    }
}

#[test]
fn buttons_report_presence_apart_from_pressing() {
    let mut pad = Gamepad {
        id: ControllerId(0),
        family: Family::Other,
        name: None,
        left_stick: Direction::default(),
        right_stick: Direction::default(),
        left_trigger: 0.,
        right_trigger: 0.,
        pressed: 0,
        present: 0,
    };
    pad.set(Button::South, true);
    pad.set(Button::Home, false);
    for button in Button::ALL {
        let expected_present = matches!(button, Button::South | Button::Home);
        assert_eq!(pad.has(button), expected_present, "{button:?}");
        assert_eq!(
            pad.is_pressed(button),
            button == Button::South,
            "{button:?}"
        );
    }
}
