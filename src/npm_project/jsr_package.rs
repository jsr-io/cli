// Copyright the JSR authors. MIT license.

use std::fmt;

use lazy_regex::regex;

/// A JSR package name with an optional version, like `@std/encoding@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsrPackage {
  pub scope: String,
  pub name: String,
  pub version: Option<String>,
}

#[derive(Debug, thiserror::Error)]
#[error(
  "Invalid jsr package name: A jsr package name must have the format @<scope>/<name>, but got \"{0}\""
)]
pub struct JsrPackageNameError(String);

impl JsrPackage {
  /// Parses `@<scope>/<name>[@<version>]`, or the npm compatible
  /// `@jsr/<scope>__<name>[@<version>]`.
  pub fn parse(input: &str) -> Result<Self, JsrPackageNameError> {
    let captures = regex!(r"^@([a-z0-9-]+)/([a-z0-9-]+)(@(.+))?$")
      .captures(input)
      .or_else(|| {
        regex!(r"^@jsr/([a-z0-9-]+)__([a-z0-9-]+)(@(.+))?$").captures(input)
      })
      .ok_or_else(|| JsrPackageNameError(input.to_string()))?;
    Ok(Self {
      scope: captures[1].to_string(),
      name: captures[2].to_string(),
      version: captures.get(4).map(|m| m.as_str().to_string()),
    })
  }

  /// The name of the package on the JSR npm registry, like
  /// `@jsr/std__encoding@1`.
  pub fn to_npm_package(&self) -> String {
    format!(
      "@jsr/{}__{}{}",
      self.scope,
      self.name,
      self.version_suffix()
    )
  }

  fn version_suffix(&self) -> String {
    match &self.version {
      Some(version) => format!("@{version}"),
      None => String::new(),
    }
  }
}

impl fmt::Display for JsrPackage {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "@{}/{}{}", self.scope, self.name, self.version_suffix())
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[test]
  fn parse() {
    let pkg = JsrPackage::parse("@std/encoding").unwrap();
    assert_eq!(pkg.scope, "std");
    assert_eq!(pkg.name, "encoding");
    assert_eq!(pkg.version, None);
    assert_eq!(pkg.to_string(), "@std/encoding");
    assert_eq!(pkg.to_npm_package(), "@jsr/std__encoding");

    let pkg = JsrPackage::parse("@std/encoding@^1.0.0").unwrap();
    assert_eq!(pkg.version.as_deref(), Some("^1.0.0"));
    assert_eq!(pkg.to_string(), "@std/encoding@^1.0.0");
    assert_eq!(pkg.to_npm_package(), "@jsr/std__encoding@^1.0.0");

    let pkg = JsrPackage::parse("@jsr/std__encoding@1").unwrap();
    assert_eq!(pkg.to_string(), "@std/encoding@1");
  }

  #[test]
  fn parse_scope_starting_with_number() {
    JsrPackage::parse("@0abc/foo").unwrap();
    JsrPackage::parse("@jsr/0abc__foo").unwrap();
  }

  #[test]
  fn parse_invalid() {
    for input in ["std/encoding", "@std", "@Std/encoding", "encoding"] {
      let err = JsrPackage::parse(input).unwrap_err();
      assert!(err.to_string().contains(input));
    }
  }
}
