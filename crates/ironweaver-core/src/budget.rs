// budget.rs
//
// Limits on how much work a search does and how much it returns, for
// callers that must bound every request (a server answering remote
// queries). Depth limits bound neither: a depth-2 traversal from a node
// with a million neighbours visits a million nodes.
//
// The `*_limited` variants of the traversals (`traversal::dfs_limited`,
// `bfs_limited`, `expand_limited`), of path expansion
// (`query::expand_paths_limited`) and of random walks
// (`WalkPlan::run_limited`) take a `Budget`. When a limit is reached they
// either fail with `GraphError::BudgetExceeded` or return what they found
// so far with `truncated` set; the budget says which. A check is a counter
// comparison per node entered or result produced.
//
// Wall-clock limits are a separate mechanism: run the search under a
// `cancel::Token`.

use crate::GraphError;

/// Limits for one search. The default has no limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    /// Most nodes the search may enter (for traversals: expand, i.e. look
    /// at their edges; for path expansion: step onto, the start included;
    /// for random walks: walk through).
    pub max_visited: Option<usize>,
    /// Most results (nodes, paths or walks) the search may produce.
    pub max_results: Option<usize>,
    /// What reaching a limit does.
    pub on_limit: OnLimit,
}

/// What a search does when it reaches a [`Budget`] limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OnLimit {
    /// Fail with [`GraphError::BudgetExceeded`].
    #[default]
    Error,
    /// Stop and return what was found so far, with
    /// [`Limited::truncated`] set.
    Truncate,
}

impl Budget {
    /// No limits.
    pub const UNLIMITED: Budget = Budget { max_visited: None, max_results: None, on_limit: OnLimit::Error };

    /// At most `n` nodes entered.
    pub fn max_visited(mut self, n: usize) -> Self {
        self.max_visited = Some(n);
        self
    }

    /// At most `n` results.
    pub fn max_results(mut self, n: usize) -> Self {
        self.max_results = Some(n);
        self
    }

    /// Return partial results instead of failing when a limit is reached.
    pub fn truncate(mut self) -> Self {
        self.on_limit = OnLimit::Truncate;
        self
    }
}

/// The result of a search run under a [`Budget`].
#[derive(Clone, Debug, PartialEq)]
pub struct Limited<T> {
    pub value: T,
    /// A limit stopped the search early (only with [`OnLimit::Truncate`]):
    /// `value` holds the first results, and more work was left. With
    /// `max_results` of `n`, `truncated` is set only if there was an
    /// `n + 1`-th result.
    pub truncated: bool,
    /// Nodes entered (see [`Budget::max_visited`]).
    pub visited: usize,
}

/// Counts a search's work against a budget.
#[derive(Debug)]
pub(crate) struct Meter {
    budget: Budget,
    visited: usize,
    results: usize,
    /// A limit was reached.
    hit: bool,
}

impl Meter {
    pub(crate) fn new(budget: Budget) -> Self {
        Meter { budget, visited: 0, results: 0, hit: false }
    }

    /// Count entering a node; false (the search must stop) if that is over
    /// the budget.
    #[inline]
    pub(crate) fn enter(&mut self) -> bool {
        if self.budget.max_visited.is_some_and(|max| self.visited >= max) {
            self.hit = true;
            return false;
        }
        self.visited += 1;
        true
    }

    /// Count producing a result; false (the search must stop, without
    /// producing it) if that is over the budget.
    #[inline]
    pub(crate) fn produce(&mut self) -> bool {
        if self.budget.max_results.is_some_and(|max| self.results >= max) {
            self.hit = true;
            return false;
        }
        self.results += 1;
        true
    }

    /// Count `n` nodes entered at once (for work bounded in advance).
    pub(crate) fn add_visited(&mut self, n: usize) {
        self.visited += n;
    }

    /// Record that a limit stopped the search.
    pub(crate) fn stopped(&mut self) {
        self.hit = true;
    }

    /// The outcome: `value`, or the error if a limit was reached in
    /// [`OnLimit::Error`] mode.
    pub(crate) fn finish<T>(self, value: T) -> Result<Limited<T>, GraphError> {
        if self.hit && self.budget.on_limit == OnLimit::Error {
            return Err(GraphError::BudgetExceeded { visited: self.visited, results: self.results });
        }
        Ok(Limited { value, truncated: self.hit, visited: self.visited })
    }
}
