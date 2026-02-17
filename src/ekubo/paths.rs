use std::collections::{HashMap, HashSet};

use super::evaluation::{Direction, Hop, Path, PoolWithTokens};
use super::swap::U256;

/// Maximum number of hops allowed in a path search.
const MAX_HOPS: usize = 3;

/// A route between two tokens through multiple pools
#[derive(Debug, Clone)]
pub struct Route {
    pub from: U256,
    pub to: U256,
    pub direction: Direction,
    pub pools: Vec<PoolWithTokens>,
}

/// Extended hop with from/to token information
#[derive(Debug, Clone)]
pub struct HopWithTokens {
    pub pool: PoolWithTokens,
    pub from: U256,
    pub to: U256,
    pub direction: Direction,
}

/// A path with token information at each hop.
pub type PathWithTokens = Vec<HopWithTokens>;

/// Get a mapping from pool key hashes to the paths that use them.
/// Returns: Map<pool_key_hash, Map<path_index, path>>
pub fn get_path_by_pools(paths: &[PathWithTokens]) -> HashMap<u64, HashMap<usize, PathWithTokens>> {
    let mut path_by_pools: HashMap<u64, HashMap<usize, PathWithTokens>> = HashMap::new();

    for (index, path) in paths.iter().enumerate() {
        for hop in path {
            let key_hash = hop.pool.key_hash();

            path_by_pools
                .entry(key_hash)
                .or_default()
                .insert(index, path.clone());
        }
    }

    path_by_pools
}

/// An entry containing a path ID and the path itself
#[derive(Debug, Clone)]
pub struct PathEntry {
    pub id: usize,
    pub path: PathWithTokens,
}

/// Get paths grouped by pool and direction.
/// Returns: Map<pool_key_hash, Map<direction, Vec<{id, path}>>>
pub fn get_paths_by_pool_directed(
    paths: &[PathWithTokens],
) -> HashMap<u64, HashMap<Direction, Vec<PathEntry>>> {
    let mut paths_by_pool: HashMap<u64, HashMap<Direction, Vec<PathEntry>>> = HashMap::new();

    for (index, path) in paths.iter().enumerate() {
        for hop in path {
            let key_hash = hop.pool.key_hash();
            let entry = PathEntry {
                id: index,
                path: path.clone(),
            };

            paths_by_pool
                .entry(key_hash)
                .or_default()
                .entry(hop.direction)
                .or_default()
                .push(entry);
        }
    }

    paths_by_pool
}

/// Convert a token address to canonical hex format for consistent hashing
fn to_canonical_hex(token: U256) -> String {
    format!("{:x}", token)
}

/// Find all paths between source and destination tokens through the given pools.
/// Uses DFS with a maximum path length of 4 hops.
pub fn get_paths<'a>(
    pools: impl IntoIterator<Item = &'a PoolWithTokens>,
    source: U256,
    destination: U256,
) -> Vec<PathWithTokens> {
    // Build adjacency list: token -> token -> Route
    let mut graph: HashMap<String, HashMap<String, Route>> = HashMap::new();

    for pool in pools {
        let t0 = to_canonical_hex(pool.token0);
        let t1 = to_canonical_hex(pool.token1);

        // Initialize entries if they don't exist
        graph.entry(t0.clone()).or_default();
        graph.entry(t1.clone()).or_default();

        // Add route from t0 to t1
        graph
            .get_mut(&t0)
            .unwrap()
            .entry(t1.clone())
            .or_insert_with(|| Route {
                from: pool.token0,
                to: pool.token1,
                direction: Direction::T0ToT1,
                pools: Vec::new(),
            })
            .pools
            .push(pool.clone());

        // Add route from t1 to t0
        graph
            .get_mut(&t1)
            .unwrap()
            .entry(t0.clone())
            .or_insert_with(|| Route {
                from: pool.token1,
                to: pool.token0,
                direction: Direction::T1ToT0,
                pools: Vec::new(),
            })
            .pools
            .push(pool.clone());
    }

    let mut result: Vec<PathWithTokens> = Vec::new();
    let mut visited_tokens: HashSet<String> = HashSet::new();
    let mut visited_pools: HashSet<u64> = HashSet::new();

    fn dfs(
        current: U256,
        destination: U256,
        hops: &mut PathWithTokens,
        graph: &HashMap<String, HashMap<String, Route>>,
        visited_tokens: &mut HashSet<String>,
        visited_pools: &mut HashSet<u64>,
        result: &mut Vec<PathWithTokens>,
    ) {
        let current_key = to_canonical_hex(current);

        let routes = match graph.get(&current_key) {
            Some(r) => r,
            None => return,
        };

        for route in routes.values() {
            let to_key = to_canonical_hex(route.to);

            if visited_tokens.contains(&to_key) {
                continue;
            }

            for pool in &route.pools {
                let pool_key_hash = pool.key_hash();

                if visited_pools.contains(&pool_key_hash) {
                    continue;
                }

                let hop = HopWithTokens {
                    pool: pool.clone(),
                    from: current,
                    to: route.to,
                    direction: route.direction,
                };

                hops.push(hop);

                if hops.len() > MAX_HOPS {
                    hops.pop();
                    continue;
                }

                // Check if we reached the destination
                if route.to == destination {
                    result.push(hops.clone());
                    hops.pop();
                    continue;
                }

                // Continue DFS
                visited_tokens.insert(to_key.clone());
                visited_pools.insert(pool_key_hash);

                dfs(
                    route.to,
                    destination,
                    hops,
                    graph,
                    visited_tokens,
                    visited_pools,
                    result,
                );

                visited_pools.remove(&pool_key_hash);
                visited_tokens.remove(&to_key);
                hops.pop();
            }
        }
    }

    let mut hops: PathWithTokens = Vec::new();
    dfs(
        source,
        destination,
        &mut hops,
        &graph,
        &mut visited_tokens,
        &mut visited_pools,
        &mut result,
    );

    result
}

/// Convert a PathWithTokens to a Path (for use with evaluate_path)
/// Returns a path with borrowed references to the pools in the input
pub fn path_with_tokens_to_path(path: &PathWithTokens) -> Path<'_> {
    path.iter()
        .map(|hop| Hop {
            direction: hop.direction,
            pool: &hop.pool,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ekubo::swap::{Pool, Tick};

    fn create_test_pool(token0: U256, token1: U256) -> PoolWithTokens {
        let pool = Pool::from_hex(
            vec![
                Tick {
                    tick: -1000,
                    delta: 5000,
                },
                Tick {
                    tick: 0,
                    delta: 10000,
                },
                Tick {
                    tick: 1000,
                    delta: 5000,
                },
            ],
            100,
            "0x6389f7f2203147955d5b12e80a8286b94becf0a",
            10000,
            "0x68db8bac710cb4000000000000000",
        )
        .unwrap();

        PoolWithTokens::new(pool, token0, token1, U256::ZERO, 0, U256::ZERO)
    }

    #[test]
    fn test_get_paths_direct() {
        let pool = create_test_pool(U256::from(1u64), U256::from(2u64));
        let pools = vec![pool];

        let paths = get_paths(&pools, U256::from(1u64), U256::from(2u64));
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].len(), 1);
        assert_eq!(paths[0][0].direction, Direction::T0ToT1);
    }

    #[test]
    fn test_get_paths_reverse_direction() {
        let pool = create_test_pool(U256::from(1u64), U256::from(2u64));
        let pools = vec![pool];

        let paths = get_paths(&pools, U256::from(2u64), U256::from(1u64));
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].len(), 1);
        assert_eq!(paths[0][0].direction, Direction::T1ToT0);
    }

    #[test]
    fn test_get_paths_two_hops() {
        let pool1 = create_test_pool(U256::from(1u64), U256::from(2u64));
        let pool2 = create_test_pool(U256::from(2u64), U256::from(3u64));
        let pools = vec![pool1, pool2];

        let paths = get_paths(&pools, U256::from(1u64), U256::from(3u64));
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].len(), 2);
    }

    #[test]
    fn test_get_paths_no_path() {
        let pool = create_test_pool(U256::from(1u64), U256::from(2u64));
        let pools = vec![pool];

        let paths = get_paths(&pools, U256::from(1u64), U256::from(3u64));
        assert!(paths.is_empty());
    }

    #[test]
    fn test_get_paths_multiple_routes() {
        // Create a triangle: 1 <-> 2 <-> 3 <-> 1
        let pool1 = create_test_pool(U256::from(1u64), U256::from(2u64));
        let pool2 = create_test_pool(U256::from(2u64), U256::from(3u64));
        let pool3 = create_test_pool(U256::from(1u64), U256::from(3u64));
        let pools = vec![pool1, pool2, pool3];

        let paths = get_paths(&pools, U256::from(1u64), U256::from(3u64));
        // Should find: direct (1->3) and via 2 (1->2->3)
        assert_eq!(paths.len(), 2);
    }

    #[test]
    fn test_get_paths_max_hops() {
        // Create a chain: 1 -> 2 -> 3 -> 4 -> 5
        // Max hops is 3 (enforced by `hops.len() > 3` in dfs)
        let pool1 = create_test_pool(U256::from(1u64), U256::from(2u64));
        let pool2 = create_test_pool(U256::from(2u64), U256::from(3u64));
        let pool3 = create_test_pool(U256::from(3u64), U256::from(4u64));
        let pool4 = create_test_pool(U256::from(4u64), U256::from(5u64));
        let pools = vec![pool1, pool2, pool3, pool4];

        // 1 to 4 should work (3 hops)
        let paths = get_paths(&pools, U256::from(1u64), U256::from(4u64));
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].len(), 3);

        // 1 to 5 should not work (would need 4 hops, exceeds max of 3)
        let paths = get_paths(&pools, U256::from(1u64), U256::from(5u64));
        assert!(paths.is_empty());
    }

    #[test]
    fn test_get_path_by_pools() {
        let pool1 = create_test_pool(U256::from(1u64), U256::from(2u64));
        let pool2 = create_test_pool(U256::from(2u64), U256::from(3u64));
        let pools = vec![pool1.clone(), pool2.clone()];

        let paths = get_paths(&pools, U256::from(1u64), U256::from(3u64));
        let path_by_pools = get_path_by_pools(&paths);

        // Both pools should be in the map
        assert!(path_by_pools.contains_key(&pool1.key_hash()));
        assert!(path_by_pools.contains_key(&pool2.key_hash()));
    }

    #[test]
    fn test_get_paths_by_pool_directed() {
        let pool = create_test_pool(U256::from(1u64), U256::from(2u64));
        let pools = vec![pool.clone()];

        // Get paths in both directions
        let paths_forward = get_paths(&pools, U256::from(1u64), U256::from(2u64));
        let paths_backward = get_paths(&pools, U256::from(2u64), U256::from(1u64));

        let all_paths: Vec<_> = paths_forward.into_iter().chain(paths_backward).collect();
        let by_pool_directed = get_paths_by_pool_directed(&all_paths);

        let pool_entry = by_pool_directed.get(&pool.key_hash()).unwrap();
        assert!(pool_entry.contains_key(&Direction::T0ToT1));
        assert!(pool_entry.contains_key(&Direction::T1ToT0));
    }

    #[test]
    fn test_path_with_tokens_to_path() {
        let pool = create_test_pool(U256::from(1u64), U256::from(2u64));
        let pools = vec![pool];

        let paths = get_paths(&pools, U256::from(1u64), U256::from(2u64));
        assert!(!paths.is_empty());

        let path = path_with_tokens_to_path(&paths[0]);
        assert_eq!(path.len(), 1);
        assert_eq!(path[0].direction, Direction::T0ToT1);
    }
}
