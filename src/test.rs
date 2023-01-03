use once_cell::sync::Lazy;
use regex::Regex;

use crate::{
    get_local_timestamp_rfc3339,
    offset_from_process,
    set_global_offset,
};

static TIME_FORMAT_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new("\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}[+|-]\\d{2}:\\d{2}").unwrap()
});

macro_rules! test_is_ok {
    ($offset_hr:expr, $offset_min:expr, $exp_ts_offset:expr) => {
        assert!(set_global_offset($offset_hr, $offset_min).is_ok());
        let res = get_local_timestamp_rfc3339();
        assert!(res.is_ok(), "res: {:#?}", res);
        let res = res.unwrap();
        assert!(!res.is_empty(), "res: {:#?}", res);
        assert!(TIME_FORMAT_REGEX.is_match(&res), "res: {:#?}", res);
        assert!(res.contains($exp_ts_offset),  "res: {:#?}", res); 
    };
}

#[test]
fn offset_tests() {
    test_is_ok!(-8, 0, "-08:00");
    test_is_ok!(6, 0, "+06:00");
    test_is_ok!(0, 0, "+00:00");
    assert!(set_global_offset(127, 0).is_err());
    assert!(set_global_offset(-127, 0).is_err());
    assert!(set_global_offset(0, -1).is_err());
    assert!(set_global_offset(0, 60).is_err());
}

#[test]
fn get_offset_from_proc_test() {
    let res = offset_from_process();
    assert!(res.is_some());
}