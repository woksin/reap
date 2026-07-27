//! The figures reap puts in front of a user, and the strings it derives them
//! from.
//!
//! Everything this tool is for comes down to a number and a unit. A reading
//! that is wrong by a factor of a million, or one that reads `0 B` because a
//! string was not understood, is not a cosmetic fault — it is the tool
//! answering the only question it was opened to answer, incorrectly, with no
//! sign that anything went wrong.

use crate::scan::docker::{parse_since, parse_size};
use crate::util::human;

mod when_stating_a_size_to_a_user {
    use super::*;

    #[test]
    fn should_leave_small_counts_in_bytes() {
        assert_eq!(human(0), "0 B");
        assert_eq!(human(1), "1 B");
        assert_eq!(human(999), "999 B");
    }

    #[test]
    fn should_divide_by_a_thousand_rather_than_by_1024() {
        // SI, so these line up with what macOS and `docker system df` report
        // and can be checked against them directly. `du -h` would say 977 kB.
        assert_eq!(human(1_000), "1.00 kB");
        assert_eq!(human(1_000_000), "1.00 MB");
        assert_eq!(human(1_000_000_000), "1.00 GB");
        assert_eq!(human(1_000_000_000_000), "1.00 TB");
    }

    #[test]
    fn should_hold_three_significant_figures_across_the_whole_range() {
        // Precision falls away as the mantissa grows, so the figure stays the
        // same width however large it is.
        assert_eq!(human(1_234), "1.23 kB");
        assert_eq!(human(12_345), "12.3 kB");
        assert_eq!(human(123_456), "123 kB");
    }

    #[test]
    fn should_step_up_a_unit_before_showing_a_four_digit_figure() {
        // 999_999 divided once is 999.999 kB, which prints without decimals
        // and so rounds to "1000 kB" — four digits, and a unit no one would
        // have chosen for it.
        assert_eq!(human(999_999), "1.00 MB");
        assert_eq!(human(999_999_999), "1.00 GB");
        // The value just below still belongs where it is.
        assert_eq!(human(999_400), "999 kB");
    }

    #[test]
    fn should_stop_at_the_largest_unit_it_knows() {
        // Rather than run off the end of the table.
        assert!(human(u64::MAX).ends_with(" PB"), "{}", human(u64::MAX));
    }
}

mod when_reading_a_size_docker_reported {
    use super::*;

    #[test]
    fn should_read_the_forms_a_real_daemon_emits() {
        // Every distinct spelling observed in `docker system df -v` output.
        assert_eq!(parse_size("0B"), Some(0));
        assert_eq!(parse_size("77.8kB"), Some(77_800));
        assert_eq!(parse_size("1.73MB"), Some(1_730_000));
        assert_eq!(parse_size("492MB"), Some(492_000_000));
        assert_eq!(parse_size("1.637GB"), Some(1_637_000_000));
    }

    #[test]
    fn should_treat_the_units_as_powers_of_a_thousand() {
        // The daemon reports SI here. Reading GB as 1024^3 would overstate
        // every image by 7%.
        assert_eq!(parse_size("1GB"), Some(1_000_000_000));
        assert_eq!(parse_size("1GiB"), Some(1_073_741_824));
    }

    #[test]
    fn should_ignore_the_marker_docker_puts_on_a_shared_figure() {
        assert_eq!(parse_size("113.7MB*"), Some(113_700_000));
    }

    #[test]
    fn should_report_no_size_where_docker_reports_none() {
        // "N/A" is docker declining to measure, which is not the same claim as
        // zero and must not be shown as one.
        assert_eq!(parse_size("N/A"), None);
        assert_eq!(parse_size(""), None);
    }

    #[test]
    fn should_refuse_a_unit_it_does_not_know_rather_than_assume_bytes() {
        // The failure this guards against: falling back to a multiplier of one
        // turns 192.4 megabytes into 192 bytes — a figure six orders of
        // magnitude out, reported with total confidence.
        assert_eq!(parse_size("192.4MB (virtual 1.75GB)"), None);
        assert_eq!(parse_size("15 quorks"), None);
    }

    #[test]
    fn should_refuse_anything_that_is_not_a_number() {
        assert_eq!(parse_size("unknown"), None);
        assert_eq!(parse_size("-4MB"), None);
        assert_eq!(parse_size("<none>"), None);
    }
}

mod when_reading_an_age_docker_reported {
    use super::*;

    #[test]
    fn should_read_the_forms_a_real_daemon_emits() {
        assert_eq!(parse_since("5 days ago"), Some(5));
        assert_eq!(parse_since("3 weeks ago"), Some(21));
        assert_eq!(parse_since("2 months ago"), Some(60));
        assert_eq!(parse_since("2 years ago"), Some(730));
    }

    #[test]
    fn should_call_anything_inside_a_day_zero_days_old() {
        assert_eq!(parse_since("42 seconds ago"), Some(0));
        assert_eq!(parse_since("13 minutes ago"), Some(0));
        assert_eq!(parse_since("9 hours ago"), Some(0));
    }

    #[test]
    fn should_read_the_buckets_docker_words_rather_than_counts() {
        // Docker rounds the short end into prose instead of a number.
        assert_eq!(parse_since("Less than a second ago"), Some(0));
        assert_eq!(parse_since("About a minute ago"), Some(0));
        assert_eq!(parse_since("About an hour ago"), Some(0));
    }

    #[test]
    fn should_report_no_age_where_it_cannot_tell() {
        // An age reap cannot read is left absent, which the interface renders
        // as a dash. Guessing would put an item in the wrong stale bucket.
        assert_eq!(parse_since(""), None);
        assert_eq!(parse_since("N/A"), None);
        assert_eq!(parse_since("7 fortnights ago"), None);
    }
}
