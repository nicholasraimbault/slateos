// Slate OS wvkbd color scheme
// Colors derived from slate-common default palette:
//   primary:   [100, 149, 237] cornflower blue
//   surface:   [18, 18, 24]    near-black with blue tint
//   container: [30, 30, 40]    slightly lighter surface
//   neutral:   [228, 228, 232] off-white text
//
// These are compile-time defaults. Runtime theming via slate-palette
// would require a wvkbd fork with config-file or D-Bus support.

#ifndef CONFIG_SLATE_H
#define CONFIG_SLATE_H

#define DEFAULT_FONT "Sans 14"
#define DEFAULT_ROUNDING 8

static const int transparency = 255;

// Scheme 0: normal keys (letters, numbers, punctuation)
// Scheme 1: special keys (Shift, Ctrl, Esc, Tab, Backspace, Enter)
struct clr_scheme schemes[] = {
{
	// Scheme 0 -- normal keys
	// bg:   surface color (keyboard background)
	// fg:   container color (key face)
	// high: primary accent (pressed state)
	// text: neutral off-white
	.bg = {.bgra = {24, 18, 18, transparency}},     // #12121A (BGRA order)
	.fg = {.bgra = {40, 30, 30, transparency}},      // #1E1E28
	.high = {.bgra = {237, 149, 100, transparency}}, // #6495ED
	.swipe = {.bgra = {237, 149, 100, 64}},          // #6495ED at 25%
	.text = {.bgra = {232, 228, 228, transparency}}, // #E4E4E8
	.text_press = {.bgra = {24, 18, 18, transparency}}, // #12121A (dark on light)
	.text_swipe = {.bgra = {232, 228, 228, transparency}},
	.font = DEFAULT_FONT,
	.rounding = DEFAULT_ROUNDING,
},
{
	// Scheme 1 -- special/modifier keys
	// Slightly lighter background, primary-colored text
	.bg = {.bgra = {24, 18, 18, transparency}},      // #12121A
	.fg = {.bgra = {56, 42, 42, transparency}},       // #2A2A38
	.high = {.bgra = {237, 149, 100, transparency}},  // #6495ED
	.swipe = {.bgra = {237, 149, 100, 64}},           // #6495ED at 25%
	.text = {.bgra = {237, 149, 100, transparency}},  // #6495ED (primary accent)
	.text_press = {.bgra = {24, 18, 18, transparency}},
	.text_swipe = {.bgra = {237, 149, 100, transparency}},
	.font = DEFAULT_FONT,
	.rounding = DEFAULT_ROUNDING,
}
};

// Only one layer -- the developer layout has everything on the main screen
static enum layout_id layers[] = {
	SlateMain,
	NumLayouts,
};

// Same layout for landscape (wvkbd requires this array)
static enum layout_id landscape_layers[] = {
	SlateMain,
	NumLayouts,
};

#endif // CONFIG_SLATE_H
