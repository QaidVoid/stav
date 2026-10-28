use super::{did_you_mean, distance};

#[test]
fn a_close_name_is_suggested() {
    assert_eq!(
        did_you_mean("nginix", ["nginx", "apache"]),
        Some("did you mean `nginx`?".to_owned())
    );
}

#[test]
fn transpositions_count_as_one_edit() {
    assert_eq!(distance("runti", "runit", 3), Some(1));
    assert_eq!(
        did_you_mean("runti", ["runit", "s6"]).as_deref(),
        Some("did you mean `runit`?")
    );
}

#[test]
fn distant_names_are_not_suggested() {
    assert_eq!(did_you_mean("port", ["hostname", "listen"]), None);
}

#[test]
fn the_limit_grows_with_the_length_of_the_name() {
    assert_eq!(did_you_mean("ab", ["xy"]), None);
    assert_eq!(
        did_you_mean("service", ["servise"]).as_deref(),
        Some("did you mean `servise`?")
    );
    assert_eq!(
        did_you_mean("abcdefghi", ["abcxyzghi"]),
        Some("did you mean `abcxyzghi`?".to_owned())
    );
}

#[test]
fn at_most_three_closest_candidates_are_suggested_in_order() {
    let help = did_you_mean("cat", ["bat", "cut", "car", "cab", "dog"]);
    assert_eq!(help.as_deref(), Some("did you mean `bat`, `cab` or `car`?"));
}

#[test]
fn the_name_itself_and_duplicates_are_not_suggested() {
    assert_eq!(did_you_mean("x", ["x"]), None);
    assert_eq!(
        did_you_mean("ab", ["ac", "ac"]).as_deref(),
        Some("did you mean `ac`?")
    );
}

#[test]
fn distances_handle_multibyte_characters() {
    assert_eq!(distance("héllo", "hello", 2), Some(1));
    assert_eq!(distance("", "ab", 2), Some(2));
    assert_eq!(distance("ab", "", 1), None);
}
