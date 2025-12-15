//! Catppuccin Mocha color theme for TUI
//!
//! Colors from: https://github.com/catppuccin/catppuccin

use ratatui::style::Color;

// Base colors
pub const BASE: Color = Color::Rgb(30, 30, 46); // #1e1e2e
pub const MANTLE: Color = Color::Rgb(24, 24, 37); // #181825
pub const CRUST: Color = Color::Rgb(17, 17, 27); // #11111b

// Surface colors
pub const SURFACE0: Color = Color::Rgb(49, 50, 68); // #313244
pub const SURFACE1: Color = Color::Rgb(69, 71, 90); // #45475a
pub const SURFACE2: Color = Color::Rgb(88, 91, 112); // #585b70

// Text colors
pub const TEXT: Color = Color::Rgb(205, 214, 244); // #cdd6f4
pub const SUBTEXT1: Color = Color::Rgb(186, 194, 222); // #bac2de
pub const SUBTEXT0: Color = Color::Rgb(166, 173, 200); // #a6adc8

// Accent colors
pub const ROSEWATER: Color = Color::Rgb(245, 224, 220); // #f5e0dc
pub const FLAMINGO: Color = Color::Rgb(242, 205, 205); // #f2cdcd
pub const PINK: Color = Color::Rgb(245, 194, 231); // #f5c2e7
pub const MAUVE: Color = Color::Rgb(203, 166, 247); // #cba6f7
pub const RED: Color = Color::Rgb(243, 139, 168); // #f38ba8
pub const MAROON: Color = Color::Rgb(235, 160, 172); // #eba0ac
pub const PEACH: Color = Color::Rgb(250, 179, 135); // #fab387
pub const YELLOW: Color = Color::Rgb(249, 226, 175); // #f9e2af
pub const GREEN: Color = Color::Rgb(166, 227, 161); // #a6e3a1
pub const TEAL: Color = Color::Rgb(148, 226, 213); // #94e2d5
pub const SKY: Color = Color::Rgb(137, 220, 235); // #89dceb
pub const SAPPHIRE: Color = Color::Rgb(116, 199, 236); // #74c7ec
pub const BLUE: Color = Color::Rgb(137, 180, 250); // #89b4fa
pub const LAVENDER: Color = Color::Rgb(180, 190, 254); // #b4befe

// Semantic colors for TUI
pub const PANEL_BORDER: Color = SURFACE1;
pub const PANEL_BORDER_ACTIVE: Color = MAUVE;
pub const STATUS_SUCCESS: Color = GREEN;
pub const STATUS_WARNING: Color = YELLOW;
pub const STATUS_ERROR: Color = RED;
pub const STATUS_PENDING: Color = BLUE;
pub const STATUS_SYNTHESIZING: Color = PEACH;
pub const LLM_PROMPT: Color = SKY;
pub const LLM_RESPONSE: Color = TEAL;
