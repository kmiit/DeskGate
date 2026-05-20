// Color parsing for fence backgrounds, borders, titles, and labels.
// Inputs come from the user's Fence config (a tiny set of named colors —
// "Red", "Blue", "Black", … plus the literal empty string for "default").
// Anything else falls through to white-with-alpha so unknown values are
// at least visible rather than invisible.

use windows::Win32::Graphics::Direct2D::Common::*;

pub fn parse_fence_color(custom: &Option<String>, alpha: f32) -> D2D1_COLOR_F {
    match custom {
        Some(c) => parse_named_color(c, alpha),
        None => D2D1_COLOR_F {
            r: 0.12,
            g: 0.13,
            b: 0.16,
            a: alpha,
        },
    }
}

pub fn parse_text_color(custom: &Option<String>) -> D2D1_COLOR_F {
    match custom {
        Some(c) => parse_named_color(c, 0.95),
        None => D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: 0.95,
        },
    }
}

fn parse_named_color(name: &str, alpha: f32) -> D2D1_COLOR_F {
    match name.to_lowercase().as_str() {
        "red" => D2D1_COLOR_F {
            r: 1.0,
            g: 0.2,
            b: 0.2,
            a: alpha,
        },
        "green" => D2D1_COLOR_F {
            r: 0.2,
            g: 0.8,
            b: 0.2,
            a: alpha,
        },
        "blue" => D2D1_COLOR_F {
            r: 0.2,
            g: 0.4,
            b: 1.0,
            a: alpha,
        },
        "teal" => D2D1_COLOR_F {
            r: 0.0,
            g: 0.8,
            b: 0.8,
            a: alpha,
        },
        "purple" => D2D1_COLOR_F {
            r: 0.6,
            g: 0.2,
            b: 0.8,
            a: alpha,
        },
        "orange" => D2D1_COLOR_F {
            r: 1.0,
            g: 0.6,
            b: 0.1,
            a: alpha,
        },
        "pink" => D2D1_COLOR_F {
            r: 1.0,
            g: 0.4,
            b: 0.7,
            a: alpha,
        },
        "yellow" => D2D1_COLOR_F {
            r: 1.0,
            g: 0.9,
            b: 0.2,
            a: alpha,
        },
        "gray" | "grey" => D2D1_COLOR_F {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: alpha,
        },
        "black" => D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: alpha,
        },
        "white" => D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: alpha,
        },
        _ => D2D1_COLOR_F {
            r: 1.0,
            g: 1.0,
            b: 1.0,
            a: alpha,
        },
    }
}
