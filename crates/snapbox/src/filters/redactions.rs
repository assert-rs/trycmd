use std::borrow::Cow;

/// Match pattern expressions, see [`Assert`][crate::Assert]
///
/// Built-in expressions:
/// - `...` on a line of its own: match multiple complete lines
/// - `[..]`: match multiple characters within a line
#[derive(Default, Clone, Debug)]
pub struct Substitutions {
    vars:
        std::collections::BTreeMap<&'static str, std::collections::BTreeSet<SubstitutedValueInner>>,
    unused: std::collections::BTreeSet<SubstitutedValueInner>,
}

impl Substitutions {
    pub fn new() -> Self {
        Default::default()
    }

    pub(crate) fn with_exe() -> Self {
        let mut substitutions = Self::new();
        substitutions
            .insert("[EXE]", std::env::consts::EXE_SUFFIX)
            .unwrap();
        substitutions
    }

    /// Insert an additional match pattern
    ///
    /// `key` must be enclosed in `[` and `]`.
    ///
    /// ```rust
    /// let mut subst = snapbox::Substitutions::new();
    /// subst.insert("[EXE]", std::env::consts::EXE_SUFFIX);
    /// ```
    ///
    /// With the `regex` feature, you can define patterns using regexes.
    /// You can choose to replace a subset of the regex by giving it the named capture group
    /// `replace`.
    ///
    /// ```rust
    /// # #[cfg(feature = "regex")] {
    /// let mut subst = snapbox::Substitutions::new();
    /// subst.insert("[OBJECT]", regex::Regex::new("(?<replace>(world|moon))").unwrap());
    /// # }
    /// ```
    pub fn insert(
        &mut self,
        key: &'static str,
        value: impl Into<SubstitutedValue>,
    ) -> Result<(), crate::Error> {
        let key = validate_key(key)?;
        let value = value.into();
        if let Some(inner) = value.inner {
            self.vars.entry(key).or_default().insert(inner);
        } else {
            self.unused.insert(SubstitutedValueInner::Str(key));
        }
        Ok(())
    }

    /// Insert additional match patterns
    ///
    /// keys must be enclosed in `[` and `]`.
    pub fn extend(
        &mut self,
        vars: impl IntoIterator<Item = (&'static str, impl Into<SubstitutedValue>)>,
    ) -> Result<(), crate::Error> {
        for (key, value) in vars {
            self.insert(key, value)?;
        }
        Ok(())
    }

    pub fn remove(&mut self, key: &'static str) -> Result<(), crate::Error> {
        let key = validate_key(key)?;
        self.vars.remove(key);
        Ok(())
    }

    /// Apply match pattern to `input`
    ///
    /// If `pattern` matches `input`, then `pattern` is returned.
    ///
    /// Otherwise, `input`, with as many patterns replaced as possible, will be returned.
    ///
    /// ```rust
    /// let subst = snapbox::Substitutions::new();
    /// let output = subst.normalize("Hello World!", "Hello [..]!");
    /// assert_eq!(output, "Hello [..]!");
    /// ```
    pub fn normalize(&self, input: &str, pattern: &str) -> String {
        normalize(input, pattern, self)
    }

    fn substitute(&self, input: &str) -> String {
        let mut input = input.to_owned();
        replace_many(
            &mut input,
            self.vars
                .iter()
                .flat_map(|(var, replaces)| replaces.iter().map(|replace| (replace, *var))),
        );
        input
    }

    fn clear<'v>(&self, pattern: &'v str) -> Cow<'v, str> {
        if !self.unused.is_empty() && pattern.contains('[') {
            let mut pattern = pattern.to_owned();
            replace_many(&mut pattern, self.unused.iter().map(|var| (var, "")));
            Cow::Owned(pattern)
        } else {
            Cow::Borrowed(pattern)
        }
    }
}

#[derive(Clone)]
pub struct SubstitutedValue {
    inner: Option<SubstitutedValueInner>,
}

#[derive(Clone, Debug)]
enum SubstitutedValueInner {
    Str(&'static str),
    String(String),
    #[cfg(feature = "regex")]
    Regex(regex::Regex),
}

impl SubstitutedValueInner {
    fn find_in(&self, buffer: &str) -> Option<std::ops::Range<usize>> {
        match self {
            Self::Str(s) => buffer.find(s).map(|offset| offset..(offset + s.len())),
            Self::String(s) => buffer.find(s).map(|offset| offset..(offset + s.len())),
            #[cfg(feature = "regex")]
            Self::Regex(r) => {
                let captures = r.captures(buffer)?;
                let m = captures.name("replace").or_else(|| captures.get(0))?;
                Some(m.range())
            }
        }
    }

    fn as_cmp(&self) -> &str {
        match self {
            Self::Str(s) => s,
            Self::String(s) => s,
            #[cfg(feature = "regex")]
            Self::Regex(s) => s.as_str(),
        }
    }
}

impl From<&'static str> for SubstitutedValue {
    fn from(inner: &'static str) -> Self {
        if inner.is_empty() {
            Self { inner: None }
        } else {
            Self {
                inner: Some(SubstitutedValueInner::String(
                    crate::filters::normalize_text(inner),
                )),
            }
        }
    }
}

impl From<String> for SubstitutedValue {
    fn from(inner: String) -> Self {
        if inner.is_empty() {
            Self { inner: None }
        } else {
            Self {
                inner: Some(SubstitutedValueInner::String(
                    crate::filters::normalize_text(&inner),
                )),
            }
        }
    }
}

impl From<&'_ String> for SubstitutedValue {
    fn from(inner: &'_ String) -> Self {
        inner.clone().into()
    }
}

impl From<Cow<'static, str>> for SubstitutedValue {
    fn from(inner: Cow<'static, str>) -> Self {
        match inner {
            Cow::Borrowed(s) => s.into(),
            Cow::Owned(s) => s.into(),
        }
    }
}

#[cfg(feature = "regex")]
impl From<regex::Regex> for SubstitutedValue {
    fn from(inner: regex::Regex) -> Self {
        Self {
            inner: Some(SubstitutedValueInner::Regex(inner)),
        }
    }
}

#[cfg(feature = "regex")]
impl From<&'_ regex::Regex> for SubstitutedValue {
    fn from(inner: &'_ regex::Regex) -> Self {
        inner.clone().into()
    }
}

impl PartialOrd for SubstitutedValueInner {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.as_cmp().partial_cmp(other.as_cmp())
    }
}

impl Ord for SubstitutedValueInner {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_cmp().cmp(other.as_cmp())
    }
}

impl PartialEq for SubstitutedValueInner {
    fn eq(&self, other: &Self) -> bool {
        self.as_cmp().eq(other.as_cmp())
    }
}

impl Eq for SubstitutedValueInner {}

/// Replacements is `(from, to)`
fn replace_many<'a>(
    buffer: &mut String,
    replacements: impl IntoIterator<Item = (&'a SubstitutedValueInner, &'a str)>,
) {
    for (var, replace) in replacements {
        let mut index = 0;
        while let Some(offset) = var.find_in(&buffer[index..]) {
            let old_range = (index + offset.start)..(index + offset.end);
            buffer.replace_range(old_range, replace);
            index += offset.start + replace.len();
        }
    }
}

fn validate_key(key: &'static str) -> Result<&'static str, crate::Error> {
    if !key.starts_with('[') || !key.ends_with(']') {
        return Err(format!("Key `{}` is not enclosed in []", key).into());
    }

    if key[1..(key.len() - 1)]
        .find(|c: char| !c.is_ascii_uppercase())
        .is_some()
    {
        return Err(format!("Key `{}` can only be A-Z but ", key).into());
    }

    Ok(key)
}

fn normalize(input: &str, pattern: &str, substitutions: &Substitutions) -> String {
    if input == pattern {
        return input.to_owned();
    }

    let input = substitutions.substitute(input);

    let mut normalized: Vec<&str> = Vec::new();
    let mut input_index = 0;
    let input_lines: Vec<_> = crate::utils::LinesWithTerminator::new(&input).collect();
    let mut pattern_lines = crate::utils::LinesWithTerminator::new(pattern).peekable();
    'outer: while let Some(pattern_line) = pattern_lines.next() {
        if is_line_elide(pattern_line) {
            if let Some(next_pattern_line) = pattern_lines.peek() {
                for (index_offset, next_input_line) in
                    input_lines[input_index..].iter().copied().enumerate()
                {
                    if line_matches(next_input_line, next_pattern_line, substitutions) {
                        normalized.push(pattern_line);
                        input_index += index_offset;
                        continue 'outer;
                    }
                }
                // Give up doing further normalization
                break;
            } else {
                // Give up doing further normalization
                normalized.push(pattern_line);
                // captured rest so don't copy remaining lines over
                input_index = input_lines.len();
                break;
            }
        } else {
            let Some(input_line) = input_lines.get(input_index) else {
                // Give up doing further normalization
                break;
            };

            if line_matches(input_line, pattern_line, substitutions) {
                input_index += 1;
                normalized.push(pattern_line);
            } else {
                // Give up doing further normalization
                break;
            }
        }
    }

    normalized.extend(input_lines[input_index..].iter().copied());
    normalized.join("")
}

fn is_line_elide(line: &str) -> bool {
    line == "...\n" || line == "..."
}

fn line_matches(mut input: &str, pattern: &str, substitutions: &Substitutions) -> bool {
    if input == pattern {
        return true;
    }

    let pattern = substitutions.clear(pattern);
    let mut sections = pattern.split("[..]").peekable();
    while let Some(section) = sections.next() {
        if let Some(remainder) = input.strip_prefix(section) {
            if let Some(next_section) = sections.peek() {
                if next_section.is_empty() {
                    input = "";
                } else if let Some(restart_index) = remainder.find(next_section) {
                    input = &remainder[restart_index..];
                }
            } else {
                return remainder.is_empty();
            }
        } else {
            return false;
        }
    }

    false
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn empty() {
        let input = "";
        let pattern = "";
        let expected = "";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn literals_match() {
        let input = "Hello\nWorld";
        let pattern = "Hello\nWorld";
        let expected = "Hello\nWorld";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn pattern_shorter() {
        let input = "Hello\nWorld";
        let pattern = "Hello\n";
        let expected = "Hello\nWorld";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn input_shorter() {
        let input = "Hello\n";
        let pattern = "Hello\nWorld";
        let expected = "Hello\n";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn all_different() {
        let input = "Hello\nWorld";
        let pattern = "Goodbye\nMoon";
        let expected = "Hello\nWorld";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn middles_diverge() {
        let input = "Hello\nWorld\nGoodbye";
        let pattern = "Hello\nMoon\nGoodbye";
        let expected = "Hello\nWorld\nGoodbye";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn elide_delimited_with_sub() {
        let input = "Hello World\nHow are you?\nGoodbye World";
        let pattern = "Hello [..]\n...\nGoodbye [..]";
        let expected = "Hello [..]\n...\nGoodbye [..]";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn leading_elide() {
        let input = "Hello\nWorld\nGoodbye";
        let pattern = "...\nGoodbye";
        let expected = "...\nGoodbye";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn trailing_elide() {
        let input = "Hello\nWorld\nGoodbye";
        let pattern = "Hello\n...";
        let expected = "Hello\n...";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn middle_elide() {
        let input = "Hello\nWorld\nGoodbye";
        let pattern = "Hello\n...\nGoodbye";
        let expected = "Hello\n...\nGoodbye";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn post_elide_diverge() {
        let input = "Hello\nSun\nAnd\nWorld";
        let pattern = "Hello\n...\nMoon";
        let expected = "Hello\nSun\nAnd\nWorld";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn post_diverge_elide() {
        let input = "Hello\nWorld\nGoodbye\nSir";
        let pattern = "Hello\nMoon\nGoodbye\n...";
        let expected = "Hello\nWorld\nGoodbye\nSir";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn inline_elide() {
        let input = "Hello\nWorld\nGoodbye\nSir";
        let pattern = "Hello\nW[..]d\nGoodbye\nSir";
        let expected = "Hello\nW[..]d\nGoodbye\nSir";
        let actual = normalize(input, pattern, &Substitutions::new());
        assert_eq!(expected, actual);
    }

    #[test]
    fn line_matches_cases() {
        let cases = [
            ("", "", true),
            ("", "[..]", true),
            ("hello", "hello", true),
            ("hello", "goodbye", false),
            ("hello", "[..]", true),
            ("hello", "he[..]", true),
            ("hello", "go[..]", false),
            ("hello", "[..]o", true),
            ("hello", "[..]e", false),
            ("hello", "he[..]o", true),
            ("hello", "he[..]e", false),
            ("hello", "go[..]o", false),
            ("hello", "go[..]e", false),
            (
                "hello world, goodbye moon",
                "hello [..], goodbye [..]",
                true,
            ),
            (
                "hello world, goodbye moon",
                "goodbye [..], goodbye [..]",
                false,
            ),
            (
                "hello world, goodbye moon",
                "goodbye [..], hello [..]",
                false,
            ),
            ("hello world, goodbye moon", "hello [..], [..] moon", true),
            (
                "hello world, goodbye moon",
                "goodbye [..], [..] moon",
                false,
            ),
            ("hello world, goodbye moon", "hello [..], [..] world", false),
        ];
        for (line, pattern, expected) in cases {
            let actual = line_matches(line, pattern, &Substitutions::new());
            assert_eq!(expected, actual, "line={:?}  pattern={:?}", line, pattern);
        }
    }

    #[test]
    fn test_validate_key() {
        let cases = [
            ("[HELLO", false),
            ("HELLO]", false),
            ("[HELLO]", true),
            ("[hello]", false),
            ("[HE  O]", false),
        ];
        for (key, expected) in cases {
            let actual = validate_key(key).is_ok();
            assert_eq!(expected, actual, "key={:?}", key);
        }
    }

    #[test]
    fn substitute_literal() {
        let input = "Hello world!";
        let pattern = "Hello [OBJECT]!";
        let mut sub = Substitutions::new();
        sub.insert("[OBJECT]", "world").unwrap();
        let actual = normalize(input, pattern, &sub);
        assert_eq!(actual, pattern);
    }

    #[test]
    fn substitute_disabled() {
        let input = "cargo";
        let pattern = "cargo[EXE]";
        let mut sub = Substitutions::new();
        sub.insert("[EXE]", "").unwrap();
        let actual = normalize(input, pattern, &sub);
        assert_eq!(actual, pattern);
    }

    #[test]
    #[cfg(feature = "regex")]
    fn substitute_regex_unnamed() {
        let input = "Hello world!";
        let pattern = "Hello [OBJECT]!";
        let mut sub = Substitutions::new();
        sub.insert("[OBJECT]", regex::Regex::new("world").unwrap())
            .unwrap();
        let actual = normalize(input, pattern, &sub);
        assert_eq!(actual, pattern);
    }

    #[test]
    #[cfg(feature = "regex")]
    fn substitute_regex_named() {
        let input = "Hello world!";
        let pattern = "Hello [OBJECT]!";
        let mut sub = Substitutions::new();
        sub.insert("[OBJECT]", regex::Regex::new("(?<replace>world)!").unwrap())
            .unwrap();
        let actual = normalize(input, pattern, &sub);
        assert_eq!(actual, pattern);
    }
}