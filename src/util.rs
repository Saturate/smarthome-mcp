/// Parses the string representation of a Python list that HA's template API returns.
/// e.g. "['kitchen', 'living_room']" -> ["kitchen", "living_room"]
pub fn parse_jinja_list(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed == "[]" {
        return Vec::new();
    }

    trimmed
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|s| {
            s.trim()
                .trim_start_matches(['\'', '"'])
                .trim_end_matches(['\'', '"'])
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_list() {
        assert_eq!(parse_jinja_list("[]"), Vec::<String>::new());
    }

    #[test]
    fn parse_single_quoted_list() {
        assert_eq!(
            parse_jinja_list("['kitchen', 'living_room']"),
            vec!["kitchen", "living_room"]
        );
    }

    #[test]
    fn parse_double_quoted_list() {
        assert_eq!(
            parse_jinja_list(r#"["kitchen", "bedroom"]"#),
            vec!["kitchen", "bedroom"]
        );
    }

    #[test]
    fn parse_with_whitespace() {
        assert_eq!(
            parse_jinja_list("  [ 'a' , 'b' ]  "),
            vec!["a", "b"]
        );
    }

    #[test]
    fn parse_single_item() {
        assert_eq!(parse_jinja_list("['only']"), vec!["only"]);
    }
}
