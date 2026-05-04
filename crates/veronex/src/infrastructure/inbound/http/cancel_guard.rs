use std::pin::Pin;
use tracing::Instrument;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::Stream;
use pin_project_lite::pin_project;

use crate::application::ports::inbound::inference_use_case::InferenceUseCase;
use crate::domain::value_objects::JobId;

/// Guard that cancels a job when dropped (client disconnect).
///
/// The cancel is safe: `cancel()` no-ops for jobs in terminal states
/// (Completed, Failed, Cancelled), so normal completions are unaffected.
struct CancelGuard {
    job_id: JobId,
    use_case: Arc<dyn InferenceUseCase>,
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        let job_id = self.job_id.clone();
        let use_case = self.use_case.clone();
        tokio::spawn(
            async move {
                match tokio::time::timeout(
                    crate::domain::constants::CANCEL_TIMEOUT,
                    use_case.cancel(&job_id),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => tracing::error!(%job_id, "cancel-on-drop failed: {e}"),
                    Err(_) => tracing::error!(%job_id, "cancel-on-drop timed out"),
                }
            }
            .instrument(tracing::info_span!("veronex.cancel_guard.spawn")),
        );
    }
}

pin_project! {
    /// Stream wrapper that cancels the associated job when dropped.
    ///
    /// Wraps SSE/NDJSON streams so that when the client disconnects (Axum
    /// drops the response body), the in-flight inference job is cancelled
    /// and GPU resources are freed.
    ///
    /// Only used on submit-and-stream endpoints (1:1 client↔job).
    /// Read-only replay endpoints (`stream_inference`, `stream_job_openai`)
    /// are NOT wrapped because multiple clients may share one job.
    pub struct CancelOnDrop<S> {
        #[pin]
        inner: S,
        _guard: CancelGuard,
    }
}

impl<S> CancelOnDrop<S> {
    pub fn new(inner: S, job_id: JobId, use_case: Arc<dyn InferenceUseCase>) -> Self {
        Self {
            inner,
            _guard: CancelGuard { job_id, use_case },
        }
    }
}

impl<S: Stream> Stream for CancelOnDrop<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project().inner.poll_next(cx)
    }
}
