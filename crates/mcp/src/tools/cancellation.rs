//! Cancellation of one tool call, shared by the direct tools and Code Mode.
//!
//! A tool call stops for one of two reasons: its deadline passes, or the client
//! sends `notifications/cancelled` for the request. rmcp reports the second
//! reason only through the request's [`CancellationToken`]; it keeps polling
//! the handler future. [`with_request_cancellation`] makes that token visible
//! to the work of the call, so the subprocess route can kill its process tree
//! and the typed route can set the `fallow-api` cancellation flag.
//!
//! The token travels as a task-local value instead of a parameter of every
//! tool function. Each direct tool runs inline in the `call_tool` future, so
//! one scope there reaches every route, and a new tool gets the same behavior
//! without a change to its signature.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use tokio_util::sync::CancellationToken;

tokio::task_local! {
    static REQUEST_CANCELLATION: CancellationToken;
}

/// Run `work` with `token` as the cancellation of the current request.
pub async fn with_request_cancellation<F: Future>(token: CancellationToken, work: F) -> F::Output {
    REQUEST_CANCELLATION.scope(token, work).await
}

/// The cancellation token of the current request, when the call runs inside
/// [`with_request_cancellation`]. Unit tests call the tools without one.
pub(super) fn request_cancellation() -> Option<CancellationToken> {
    REQUEST_CANCELLATION.try_with(CancellationToken::clone).ok()
}

/// Resolve when the client cancels the request. Without a request token the
/// future never resolves, so a `select!` arm on it never fires.
pub(super) async fn request_cancelled(token: Option<&CancellationToken>) {
    match token {
        Some(token) => token.cancelled().await,
        None => std::future::pending().await,
    }
}

/// In-process calls that outlived their deadline or their request and are
/// still winding down. Direct tools and Code Mode count into this one total,
/// so Code Mode does not stack a new in-process analysis on one that a direct
/// tool gave up on, or the other way round.
pub(super) static ABANDONED_API_CALLS: AbandonedApiCalls = AbandonedApiCalls::new();

const API_CALL_RUNNING: u8 = 0;
const API_CALL_FINISHED: u8 = 1;
const API_CALL_ABANDONED: u8 = 2;

/// How many abandoned in-process analyses the server carries before Code Mode
/// stops starting new ones. One is the whole point: the next host call takes
/// the killable subprocess instead of stacking a second analysis on top of one
/// that is still winding down.
const MAX_ABANDONED_API_CALLS: usize = 1;

/// Running total of in-process calls nobody is waiting for any more.
pub(super) struct AbandonedApiCalls(AtomicUsize);

impl AbandonedApiCalls {
    pub(super) const fn new() -> Self {
        Self(AtomicUsize::new(0))
    }

    pub(super) fn saturated(&self) -> bool {
        self.0.load(Ordering::SeqCst) >= MAX_ABANDONED_API_CALLS
    }

    /// Start the accounting for one in-process call. The worker keeps the
    /// returned completion and drops it when the call ends; the waiter calls
    /// [`ApiCallState::abandon`] when it stops waiting.
    pub(super) fn start(&'static self) -> (ApiCallState, ApiCallCompletion) {
        let state = Arc::new(AtomicU8::new(API_CALL_RUNNING));
        (
            ApiCallState {
                state: Arc::clone(&state),
                abandoned: self,
            },
            ApiCallCompletion {
                state,
                abandoned: self,
            },
        )
    }
}

/// Waiter-side half of the accounting.
pub(super) struct ApiCallState {
    state: Arc<AtomicU8>,
    abandoned: &'static AbandonedApiCalls,
}

impl ApiCallState {
    /// Claim the call as abandoned. The swap decides the race with the worker:
    /// whoever gets there first owns the accounting, so a call that finished
    /// just after the deadline is never counted.
    pub(super) fn abandon(&self) {
        if self.state.swap(API_CALL_ABANDONED, Ordering::SeqCst) == API_CALL_RUNNING {
            self.abandoned.0.fetch_add(1, Ordering::SeqCst);
        }
    }
}

/// Worker-side half of the accounting. Dropping it publishes the call as
/// finished and releases the slot when the waiter already gave up on it, so a
/// panicking call cannot strand the count.
pub(super) struct ApiCallCompletion {
    state: Arc<AtomicU8>,
    abandoned: &'static AbandonedApiCalls,
}

impl Drop for ApiCallCompletion {
    fn drop(&mut self) {
        if self.state.swap(API_CALL_FINISHED, Ordering::SeqCst) == API_CALL_ABANDONED {
            self.abandoned.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_request_token_is_visible_inside_the_scope_only() {
        assert!(request_cancellation().is_none());
        let token = CancellationToken::new();
        let seen = with_request_cancellation(token.clone(), async {
            request_cancellation().expect("the scope sets the token")
        })
        .await;
        token.cancel();
        assert!(seen.is_cancelled(), "the scope hands out the request token");
    }

    #[test]
    fn a_call_that_finishes_before_the_waiter_gives_up_is_never_counted() {
        static CALLS: AbandonedApiCalls = AbandonedApiCalls::new();
        let (state, completion) = CALLS.start();
        drop(completion);
        state.abandon();
        assert!(!CALLS.saturated());
    }

    #[test]
    fn an_abandoned_call_holds_its_slot_until_the_worker_ends() {
        static CALLS: AbandonedApiCalls = AbandonedApiCalls::new();
        let (state, completion) = CALLS.start();
        state.abandon();
        assert!(CALLS.saturated());
        drop(completion);
        assert!(!CALLS.saturated());
    }
}
