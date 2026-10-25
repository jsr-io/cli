// Copyright 2018-2026 the Deno authors. MIT license.

/// A function that converts a float to a string the represents a human
/// readable version of that number.
pub fn human_size(size: f64) -> String {
  let negative = if size.is_sign_positive() { "" } else { "-" };
  let size = size.abs();
  let units = ["B", "KB", "MB", "GB", "TB", "PB", "EB", "ZB", "YB"];
  if size < 1_f64 {
    return format!("{}{}{}", negative, size, "B");
  }
  let delimiter = 1024_f64;
  let exponent = std::cmp::min(
    (size.ln() / delimiter.ln()).floor() as i32,
    (units.len() - 1) as i32,
  );
  let pretty_bytes = format!("{:.2}", size / delimiter.powi(exponent))
    .parse::<f64>()
    .unwrap()
    * 1_f64;
  let unit = units[exponent as usize];
  format!("{negative}{pretty_bytes}{unit}")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_human_size() {
    assert_eq!(human_size(1_f64), "1B");
    assert_eq!(human_size((12 * 1024) as f64), "12KB");
    assert_eq!(human_size((24_i64 * 1024 * 1024) as f64), "24MB");
    assert_eq!(human_size((24_i64 * 1024 * 1024 * 1024) as f64), "24GB");
    assert_eq!(
      human_size((24_i64 * 1024 * 1024 * 1024 * 1024) as f64),
      "24TB"
    );
    assert_eq!(human_size(0_f64), "0B");
    assert_eq!(human_size(-10_f64), "-10B");
  }
}
