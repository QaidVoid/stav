use super::{components, cycles};

#[test]
fn components_list_dependencies_first() {
    let adj = vec![vec![1], vec![2], vec![], vec![0]];
    assert_eq!(components(&adj), vec![vec![2], vec![1], vec![0], vec![3]]);
}

#[test]
fn a_cycle_is_one_component() {
    let adj = vec![vec![1], vec![2], vec![0], vec![]];
    assert_eq!(components(&adj), vec![vec![0, 1, 2], vec![3]]);
}

#[test]
fn cycles_start_at_their_smallest_node_in_edge_order() {
    let adj = vec![vec![], vec![3], vec![1], vec![2]];
    assert_eq!(cycles(&adj), vec![vec![1, 3, 2]]);
}

#[test]
fn a_self_loop_is_a_cycle_and_a_lone_node_is_not() {
    let adj = vec![vec![0], vec![]];
    assert_eq!(cycles(&adj), vec![vec![0]]);
}

#[test]
fn every_component_with_a_cycle_reports_one() {
    let adj = vec![vec![1], vec![0], vec![3], vec![2, 4], vec![]];
    assert_eq!(cycles(&adj), vec![vec![0, 1], vec![2, 3]]);
}

#[test]
fn cycles_take_the_shortest_way_back() {
    let adj = vec![vec![1, 2], vec![2], vec![0]];
    assert_eq!(cycles(&adj), vec![vec![0, 2]]);
}

#[test]
fn long_chains_do_not_overflow_the_stack() {
    let n = 200_000;
    let mut adj: Vec<Vec<usize>> = (0..n).map(|i| vec![i + 1]).collect();
    adj[n - 1] = vec![0];
    let found = cycles(&adj);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].len(), n);
}

#[test]
fn a_diamond_has_no_cycle_and_lists_its_bottom_first() {
    let adj = vec![vec![1, 2], vec![3], vec![3], vec![]];
    assert_eq!(cycles(&adj), Vec::<Vec<usize>>::new());
    assert_eq!(components(&adj), vec![vec![3], vec![1], vec![2], vec![0]]);
}

#[test]
fn duplicate_edges_change_nothing() {
    let adj = vec![vec![1, 1], vec![0, 0, 2], vec![]];
    assert_eq!(components(&adj), vec![vec![2], vec![0, 1]]);
    assert_eq!(cycles(&adj), vec![vec![0, 1]]);
}

#[test]
fn a_self_loop_inside_a_larger_component_is_not_the_cycle_reported() {
    let adj = vec![vec![1], vec![1, 2], vec![0]];
    assert_eq!(cycles(&adj), vec![vec![0, 1, 2]]);
}

#[test]
fn many_small_cycles_are_found_in_linear_time() {
    let pairs = 100_000;
    let adj: Vec<Vec<usize>> = (0..2 * pairs).map(|i| vec![i ^ 1]).collect();
    let start = std::time::Instant::now();
    let found = cycles(&adj);
    assert_eq!(found.len(), pairs);
    assert_eq!(found[1], vec![2, 3]);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}
