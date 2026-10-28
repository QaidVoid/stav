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
