// Slate OS custom wvkbd layout
// Developer-optimized: number row always visible, Tab/Ctrl/Esc accessible
//
// 5 rows:
//   1. Numbers + Backspace (always visible, no long-press needed)
//   2. QWERTY
//   3. Home row with Tab
//   4. Bottom row with Shift + punctuation
//   5. Dev power row: Ctrl, Esc, pipe, slash, space, dash, underscore, semicolon, Enter

#ifndef LAYOUT_SLATE_H
#define LAYOUT_SLATE_H

#include "keyboard.h"

/* constants required by keyboard.c */
#define KBD_PIXEL_HEIGHT 280
#define KBD_PIXEL_LANDSCAPE_HEIGHT 200
#define KBD_KEY_BORDER 2

enum layout_id {
	SlateMain = 0,
	Index,
	NumLayouts,
};

// Row 1 -- Number row (always visible, unlike stock mobile keyboards)
// Row 2 -- QWERTY
// Row 3 -- Home row with Tab for indentation / terminal completion
// Row 4 -- Dev-essential keys with Shift
// Row 5 -- Dev power row: Ctrl, Esc, pipe, slash, space, dash, underscore, semicolon, Enter
static struct key keys_slate[] = {
	// Row 1: numbers + backspace
	{"1", "!", 1.0, Code, KEY_1},
	{"2", "@", 1.0, Code, KEY_2},
	{"3", "#", 1.0, Code, KEY_3},
	{"4", "$", 1.0, Code, KEY_4},
	{"5", "%", 1.0, Code, KEY_5},
	{"6", "^", 1.0, Code, KEY_6},
	{"7", "&", 1.0, Code, KEY_7},
	{"8", "*", 1.0, Code, KEY_8},
	{"9", "(", 1.0, Code, KEY_9},
	{"0", ")", 1.0, Code, KEY_0},
	{"\u232b", "\u232b", 1.5, Code, KEY_BACKSPACE, .scheme = 1},
	{"", "", 0.0, EndRow},

	// Row 2: QWERTY
	{"q", "Q", 1.0, Code, KEY_Q},
	{"w", "W", 1.0, Code, KEY_W},
	{"e", "E", 1.0, Code, KEY_E},
	{"r", "R", 1.0, Code, KEY_R},
	{"t", "T", 1.0, Code, KEY_T},
	{"y", "Y", 1.0, Code, KEY_Y},
	{"u", "U", 1.0, Code, KEY_U},
	{"i", "I", 1.0, Code, KEY_I},
	{"o", "O", 1.0, Code, KEY_O},
	{"p", "P", 1.0, Code, KEY_P},
	{"", "", 0.0, EndRow},

	// Row 3: Tab + home row
	{"Tab", "Tab", 1.5, Code, KEY_TAB, .scheme = 1},
	{"a", "A", 1.0, Code, KEY_A},
	{"s", "S", 1.0, Code, KEY_S},
	{"d", "D", 1.0, Code, KEY_D},
	{"f", "F", 1.0, Code, KEY_F},
	{"g", "G", 1.0, Code, KEY_G},
	{"h", "H", 1.0, Code, KEY_H},
	{"j", "J", 1.0, Code, KEY_J},
	{"k", "K", 1.0, Code, KEY_K},
	{"l", "L", 1.0, Code, KEY_L},
	{"", "", 0.0, EndRow},

	// Row 4: Shift + bottom row + punctuation
	{"\u21e7", "\u21e7", 1.5, Mod, Shift, .scheme = 1},
	{"z", "Z", 1.0, Code, KEY_Z},
	{"x", "X", 1.0, Code, KEY_X},
	{"c", "C", 1.0, Code, KEY_C},
	{"v", "V", 1.0, Code, KEY_V},
	{"b", "B", 1.0, Code, KEY_B},
	{"n", "N", 1.0, Code, KEY_N},
	{"m", "M", 1.0, Code, KEY_M},
	{",", "<", 1.0, Code, KEY_COMMA},
	{".", ">", 1.0, Code, KEY_DOT},
	{"\u21e7", "\u21e7", 1.5, Mod, Shift, .scheme = 1},
	{"", "", 0.0, EndRow},

	// Row 5: dev power row -- modifiers and special chars for coding/terminal
	{"Ctrl", "Ctrl", 1.5, Mod, Ctrl, .scheme = 1},
	{"Esc", "Esc", 1.0, Code, KEY_ESC, .scheme = 1},
	{"|", "\\", 1.0, Code, KEY_BACKSLASH},
	{"/", "?", 1.0, Code, KEY_SLASH},
	{"", "", 4.0, Code, KEY_SPACE},
	{"-", "_", 1.0, Code, KEY_MINUS},
	{"=", "+", 1.0, Code, KEY_EQUAL},
	{";", ":", 1.0, Code, KEY_SEMICOLON},
	{"\u23ce", "\u23ce", 1.5, Code, KEY_ENTER, .scheme = 1},
	{"", "", 0.0, Last},
};

// Forward-declare keys_index so layouts[] can reference it.
// keys_index is defined after layouts[] so it can take &layouts[SlateMain].
static struct key keys_index[];

static struct layout layouts[NumLayouts] = {
	[SlateMain] = {keys_slate, "latin", "slate", true},
	[Index] = {keys_index, "latin", "index", false},
};

// Index layout -- wvkbd requires this for the layout-switcher.
// Single-layout keyboard: just point back to SlateMain.
static struct key keys_index[] = {
	{"Slate", "Slate", 1.0, Layout, 0, &layouts[SlateMain], .scheme = 1},
	{"", "", 0.0, Last},
};

#endif // LAYOUT_SLATE_H
