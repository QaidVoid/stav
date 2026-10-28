//! Strongly connected components and cycles of small directed graphs.
//!
//! Graphs are adjacency lists over the nodes `0..n`. Every function here is
//! iterative, so deep graphs cannot overflow the stack, and deterministic,
//! because it visits nodes and edges in index order.

/// Returns the strongly connected components of the graph, each sorted, in
/// an order where every component comes after the components it has edges
/// to. When edges point from a dependent node to its dependencies, the
/// order therefore lists dependencies first.
pub(crate) fn components(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    const UNSET: usize = usize::MAX;
    let n = adj.len();
    let mut index = vec![UNSET; n];
    let mut low = vec![0; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut out = Vec::new();
    let mut next = 0;
    for start in 0..n {
        if index[start] != UNSET {
            continue;
        }
        index[start] = next;
        low[start] = next;
        next += 1;
        stack.push(start);
        on_stack[start] = true;
        let mut frames = vec![(start, 0usize)];
        while let Some(&(v, i)) = frames.last() {
            if let Some(&w) = adj[v].get(i) {
                if let Some(frame) = frames.last_mut() {
                    frame.1 += 1;
                }
                if w >= n {
                    continue;
                }
                if index[w] == UNSET {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on_stack[w] = true;
                    frames.push((w, 0));
                } else if on_stack[w] {
                    low[v] = low[v].min(index[w]);
                }
                continue;
            }
            frames.pop();
            if let Some(&(u, _)) = frames.last() {
                low[u] = low[u].min(low[v]);
            }
            if low[v] == index[v] {
                let mut component = Vec::new();
                while let Some(w) = stack.pop() {
                    on_stack[w] = false;
                    component.push(w);
                    if w == v {
                        break;
                    }
                }
                component.sort_unstable();
                out.push(component);
            }
        }
    }
    out
}

/// Returns one cycle for every strongly connected component that has one,
/// as the list of its nodes in edge order. A cycle starts at the smallest
/// node of its component and is as short as possible from there. A node
/// with an edge to itself is a cycle of one node.
pub(crate) fn cycles(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let mut out = Vec::new();
    for component in components(adj) {
        let start = component[0];
        if component.len() == 1 {
            if adj[start].contains(&start) {
                out.push(vec![start]);
            }
            continue;
        }
        let mut parent = vec![usize::MAX; adj.len()];
        let mut queue = std::collections::VecDeque::from([start]);
        let mut last = None;
        'search: while let Some(v) = queue.pop_front() {
            for &w in &adj[v] {
                if w == start {
                    last = Some(v);
                    break 'search;
                }
                if w < adj.len() && parent[w] == usize::MAX && component.binary_search(&w).is_ok() {
                    parent[w] = v;
                    queue.push_back(w);
                }
            }
        }
        let Some(mut node) = last else { continue };
        let mut cycle = vec![node];
        while node != start {
            node = parent[node];
            cycle.push(node);
        }
        cycle.reverse();
        out.push(cycle);
    }
    out
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;
