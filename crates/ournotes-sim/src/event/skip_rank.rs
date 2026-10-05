//! Fixed Skip result rank selected from the client's master parameter.
//!
//! The JP Skip result initializes the out rank to None (0) before TryParse. Event/Challenge-point settlement
//! reads that rank even when parsing fails. EXP has a separate success-dependent override; retain `parsed`
//! so a caller can preserve its original score-derived EXP on failure instead of inventing a shared fallback.

use crate::error::Error;
use crate::master::Master;

const PARAMETER: &str = "live_skip_result_score_rank";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkipResultRank {
    pub rank: i64,
    pub parsed: bool,
}

impl SkipResultRank {
    const FAILED: Self = Self { rank: 0, parsed: false };

    fn parsed(rank: i32) -> Self {
        Self { rank: i64::from(rank), parsed: true }
    }

    /// EXP first resolves a score-derived row and overrides its rank only after successful enum parsing.
    /// Event and Challenge-point counters always use `rank`, including None after parse failure.
    pub fn exp_rank(self, score_rank: i64) -> i64 {
        if self.parsed { self.rank } else { score_rank }
    }
}

/// Read the required native parameter. Absence is a master lookup failure, distinct from TryParse failure.
pub fn skip_result_rank(master: &Master) -> Result<SkipResultRank, Error> {
    let row = master
        .parameters
        .iter()
        .find(|row| row.id == PARAMETER)
        .ok_or_else(|| Error::Master(format!("missing parameter {PARAMETER}")))?;
    if row.value_type.as_deref() != Some("String") {
        return Err(Error::Master(format!("parameter {PARAMETER} requires native String type")));
    }
    Ok(parse_result_rank(&row.value))
}

/// Case-sensitive enum names (comma-separated names combine with bitwise OR), or one signed decimal Int32
/// token. Undefined numeric values also succeed: TryParse does not require a declared name or reward row.
fn parse_result_rank(value: &str) -> SkipResultRank {
    let value = value.trim();
    if value.starts_with(|c: char| c.is_ascii_digit() || c == '+' || c == '-') {
        return value.parse::<i32>().map_or(SkipResultRank::FAILED, SkipResultRank::parsed);
    }
    let mut rank = 0;
    for name in value.split(',') {
        rank |= match name.trim() {
            "None" => 0,
            "E" => 1,
            "D" => 2,
            "C" => 3,
            "B" => 4,
            "A" => 5,
            "S" => 6,
            "SS" => 7,
            _ => return SkipResultRank::FAILED,
        };
    }
    SkipResultRank::parsed(rank)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn master_parameter(value: Option<&str>) -> Master {
        let text = json!({"_allData":value.map_or_else(Vec::new, |value| {
            vec![json!({"_id":PARAMETER,"_type":"String","_value":value})]
        })})
        .to_string();
        Master::from_json_tables(|name| (name == "MasterParameter").then_some(text.as_str())).unwrap()
    }

    #[test]
    fn case_sensitive_enum_names_keep_parse_success_separate_from_none() {
        for (rank, name) in ["None", "E", "D", "C", "B", "A", "S", "SS"].into_iter().enumerate() {
            assert_eq!(parse_result_rank(name), SkipResultRank { rank: rank as i64, parsed: true });
        }
        for value in ["none", "NONE", "c", "ss", "Ss", "LiveScoreRank.C", "unknown", "", " \t\r\n "] {
            assert_eq!(parse_result_rank(value), SkipResultRank::FAILED, "{value:?}");
        }
        assert_ne!(parse_result_rank("None"), parse_result_rank("unknown"));
    }

    #[test]
    fn decimal_int32_tokens_accept_signs_boundaries_and_undefined_enum_values() {
        for (text, rank) in [
            ("0", 0),
            ("-0", 0),
            ("+0", 0),
            ("003", 3),
            ("+003", 3),
            ("8", 8),
            ("-1", -1),
            ("2147483647", i32::MAX),
            ("-2147483648", i32::MIN),
        ] {
            assert_eq!(parse_result_rank(text), SkipResultRank::parsed(rank), "{text:?}");
        }
        for value in [
            "2147483648",
            "-2147483649",
            "9999999999999999999999999",
            "+",
            "-",
            "++3",
            "--3",
            "+-3",
            "3.0",
            "3e0",
            "0x3",
            "1_000",
            "3 0",
            "+ 3",
            "３",
            "٣",
        ] {
            assert_eq!(parse_result_rank(value), SkipResultRank::FAILED, "{value:?}");
        }
    }

    #[test]
    fn surrounding_whitespace_is_trimmed_without_normalizing_token_content() {
        for value in [" C ", "\tSS\r\n", "\u{00a0}C\u{2003}", "\u{3000}+3\u{3000}"] {
            let expected = if value.contains("SS") { 7 } else { 3 };
            assert_eq!(parse_result_rank(value), SkipResultRank::parsed(expected), "{value:?}");
        }
        assert_eq!(parse_result_rank("S S"), SkipResultRank::FAILED);
    }

    #[test]
    fn comma_names_use_bitwise_or_without_accepting_numeric_or_empty_name_tokens() {
        for (text, rank) in [
            ("D, E", 3),
            ("C,B", 7),
            ("E,E", 1),
            ("None, D", 2),
            (" A , E ", 5),
            ("None,None", 0),
            ("None,E,D,C,B,A,S,SS", 7),
        ] {
            assert_eq!(parse_result_rank(text), SkipResultRank::parsed(rank), "{text:?}");
        }
        for text in [",", ",C", "C,", "D,,E", "D, ,E", "D,missing", "D,e", "D,1", "1,D", "1,2"] {
            assert_eq!(parse_result_rank(text), SkipResultRank::FAILED, "{text:?}");
        }
    }

    #[test]
    fn missing_parameter_is_a_master_error_but_malformed_parameter_retains_zero_out_value() {
        assert!(matches!(skip_result_rank(&master_parameter(None)), Err(Error::Master(_))));
        assert_eq!(skip_result_rank(&master_parameter(Some("C"))).unwrap(), SkipResultRank::parsed(3));
        for invalid in ["", "broken", "2147483648"] {
            assert_eq!(skip_result_rank(&master_parameter(Some(invalid))).unwrap(), SkipResultRank::FAILED);
        }
        assert_eq!(skip_result_rank(&master_parameter(Some("None"))).unwrap(), SkipResultRank::parsed(0));
    }

    #[test]
    fn wrong_or_missing_parameter_type_is_not_enum_parse_failure() {
        for value_type in [None, Some("Int32"), Some("string")] {
            let mut master = master_parameter(Some("C"));
            master.parameters[0].value_type = value_type.map(str::to_owned);
            assert!(matches!(skip_result_rank(&master), Err(Error::Master(_))));
        }
    }

    #[test]
    fn exp_override_only_happens_after_successful_enum_parse() {
        let fixed = skip_result_rank(&master_parameter(Some("C"))).unwrap();
        let failed = skip_result_rank(&master_parameter(Some("c"))).unwrap();
        assert_eq!((fixed.rank, fixed.exp_rank(7)), (3, 3));
        assert_eq!((failed.rank, failed.exp_rank(7)), (0, 7));
    }
}
