/*!
 * Secret redaction for terminal output and stored history (Part 9, Part 14).
 *
 * Applied before command output is written to a Mission's history, streamed to
 * a shell, or handed back to a model as tool output — so a credential echoed by
 * a build script does not end up in the transcript or in a model's context.
 *
 * This is pattern matching, not a guarantee. It catches the common shapes; a
 * secret in an unusual format can still slip through.
 */

use std::sync::OnceLock;

use regex::Regex;

struct Rule {
    pattern: Regex,
    replacement: &'static str,
}

fn rules() -> &'static Vec<Rule> {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let specs: &[(&str, &str)] = &[
            // key=value / key: value forms for common credential names
            (
                r"(?i)\b(api[_-]?key|secret|token|passwd|password|access[_-]?key|private[_-]?key)\b(\s*[:=]\s*)(\S+)",
                "$1$2***",
            ),
            // Provider-issued key formats, which are recognisable on their own
            (r"sk-ant-[A-Za-z0-9\-_]{16,}", "sk-ant-***"),
            (r"sk-[A-Za-z0-9]{20,}", "sk-***"),
            (r"gh[pousr]_[A-Za-z0-9]{20,}", "gh*_***"),
            (r"AIza[0-9A-Za-z\-_]{30,}", "AIza***"),
            (r"xox[baprs]-[A-Za-z0-9\-]{10,}", "xox*-***"),
            (r"AKIA[0-9A-Z]{16}", "AKIA***"),
            // Bearer tokens in headers echoed by curl -v and friends
            (r"(?i)(authorization\s*:\s*bearer\s+)(\S+)", "${1}***"),
        ];
        specs
            .iter()
            .filter_map(|(p, r)| {
                Regex::new(p).ok().map(|pattern| Rule {
                    pattern,
                    replacement: r,
                })
            })
            .collect()
    })
}

/// Replace recognisable credentials in `input` with a masked form.
pub fn redact_secrets(input: &str) -> String {
    let mut output = input.to_string();
    for rule in rules() {
        output = rule
            .pattern
            .replace_all(&output, rule.replacement)
            .to_string();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::redact_secrets;

    #[test]
    fn masks_key_value_pairs() {
        let out = redact_secrets("API_KEY=abcd1234efgh5678");
        assert!(!out.contains("abcd1234efgh5678"), "{out}");
        assert!(out.contains("***"), "{out}");
    }

    #[test]
    fn masks_anthropic_and_openai_keys() {
        let out = redact_secrets("using sk-ant-api03-AAAAAAAAAAAAAAAAAAAA now");
        assert!(!out.contains("AAAAAAAAAAAAAAAAAAAA"), "{out}");

        let out = redact_secrets("key sk-abcdefghijklmnopqrstuvwxyz012345");
        assert!(!out.contains("abcdefghijklmnopqrstuvwxyz"), "{out}");
    }

    #[test]
    fn masks_github_tokens_of_every_prefix() {
        for prefix in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
            let raw = format!("{prefix}0123456789abcdefghijklmnopqrstuvwxyz");
            let out = redact_secrets(&raw);
            assert!(
                !out.contains("0123456789abcdef"),
                "{prefix} not masked: {out}"
            );
        }
    }

    #[test]
    fn masks_google_slack_and_aws_keys() {
        let out = redact_secrets("AIzaSyA1234567890abcdefghijklmnopqrstuv");
        assert!(!out.contains("1234567890abcdefghij"), "{out}");

        let out = redact_secrets("xoxb-123456789012-abcdefghijkl");
        assert!(!out.contains("abcdefghijkl"), "{out}");

        let out = redact_secrets("AKIAIOSFODNN7EXAMPLE");
        assert!(!out.contains("IOSFODNN7EXAMPLE"), "{out}");
    }

    #[test]
    fn masks_bearer_authorization_headers() {
        let out = redact_secrets("Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig");
        assert!(!out.contains("eyJhbGciOiJIUzI1NiJ9"), "{out}");
    }

    #[test]
    fn leaves_ordinary_output_untouched() {
        let input = "Compiling cid-core v0.1.0\n    Finished in 3.2s\n";
        assert_eq!(redact_secrets(input), input);
    }

    #[test]
    fn is_case_insensitive_for_named_secrets() {
        let out = redact_secrets("Password: hunter2hunter2");
        assert!(!out.contains("hunter2hunter2"), "{out}");
    }
}
