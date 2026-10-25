// Copyright 2018-2026 the Deno authors. MIT license.

use std::borrow::Cow;

use unicode_general_category::GeneralCategory;
use unicode_general_category::get_general_category;

pub fn escape_terminal_control_chars(text: &str) -> Cow<'_, str> {
  if !text.contains(is_terminal_unprintable) {
    return Cow::Borrowed(text);
  }

  let mut output = String::with_capacity(text.len());
  for c in text.chars() {
    if is_terminal_unprintable(c) {
      output.extend(c.escape_debug());
    } else {
      output.push(c);
    }
  }
  Cow::Owned(output)
}

fn is_terminal_unprintable(c: char) -> bool {
  matches!(
    get_general_category(c),
    GeneralCategory::Control
      | GeneralCategory::Format
      | GeneralCategory::Surrogate
      | GeneralCategory::PrivateUse
      | GeneralCategory::Unassigned
      | GeneralCategory::LineSeparator
      | GeneralCategory::ParagraphSeparator
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn escape_terminal_control_chars_preserves_printable_unicode() {
    let input = "hello 文件 😀";
    let result = escape_terminal_control_chars(input);
    assert!(matches!(result, Cow::Borrowed(_)));
    assert_eq!(result, input);
  }

  #[test]
  fn escape_terminal_control_chars_makes_controls_visible() {
    let input = "before\x1b[2J\x07\t\n\u{009b}31mafter";
    let result = escape_terminal_control_chars(input);
    assert_eq!(result, r"before\u{1b}[2J\u{7}\t\n\u{9b}31mafter");
  }

  #[test]
  fn escape_terminal_control_chars_makes_invisible_chars_visible() {
    let controls = [
      '\u{061c}',
      '\u{200e}',
      '\u{200f}',
      '\u{202a}',
      '\u{202b}',
      '\u{202c}',
      '\u{202d}',
      '\u{202e}',
      '\u{2066}',
      '\u{2067}',
      '\u{2068}',
      '\u{2069}',
      '\u{2028}',
      '\u{2029}',
      '\u{feff}',
      '\u{200b}',
      '\u{e0000}',
      '\u{e007f}',
      '\u{e000}',
      '\u{0378}',
    ];
    for control in controls {
      let input = format!("before{control}after");
      let result = escape_terminal_control_chars(&input);
      assert!(!result.contains(control));
      assert!(result.contains(&format!(r"\u{{{:x}}}", control as u32)));
    }
  }
}
