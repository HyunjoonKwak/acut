use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_cron_scheduler::{Job, JobScheduler};

use crate::core::config::ScheduleConfig;

pub struct SchedulerManager {
    scheduler: Mutex<Option<JobScheduler>>,
    active_jobs: Mutex<HashMap<String, uuid::Uuid>>,
}

impl SchedulerManager {
    pub fn new() -> Self {
        Self {
            scheduler: Mutex::new(None),
            active_jobs: Mutex::new(HashMap::new()),
        }
    }

    /// Create the internal JobScheduler and start it.
    pub async fn init(&self) {
        let sched = JobScheduler::new()
            .await
            .expect("Failed to create JobScheduler");
        sched.start().await.expect("Failed to start JobScheduler");
        let mut guard = self.scheduler.lock().await;
        *guard = Some(sched);
        log::info!("[Scheduler] initialized and started");
    }

    /// Register a new cron job. `callback` is invoked with the schedule ID
    /// each time the cron expression fires.
    pub async fn add_schedule(
        &self,
        config: &ScheduleConfig,
        callback: impl Fn(String) + Send + Sync + 'static,
    ) -> Result<(), String> {
        if !config.enabled {
            return Err(format!(
                "Schedule '{}' is disabled, not adding",
                config.id
            ));
        }

        let sched_guard = self.scheduler.lock().await;
        let scheduler = sched_guard
            .as_ref()
            .ok_or_else(|| "Scheduler not initialized – call init() first".to_string())?;

        let schedule_id = config.id.clone();
        let cron_expr = config.cron_expression.clone();
        let callback = Arc::new(callback);

        let job = Job::new_async(cron_expr.as_str(), move |_uuid, _lock| {
            let id = schedule_id.clone();
            let cb = Arc::clone(&callback);
            Box::pin(async move {
                log::info!("[Scheduler] firing schedule: {}", id);
                cb(id);
            })
        })
        .map_err(|e| format!("Invalid cron expression '{}': {}", config.cron_expression, e))?;

        let job_id = scheduler
            .add(job)
            .await
            .map_err(|e| format!("Failed to add job: {}", e))?;

        let mut jobs = self.active_jobs.lock().await;
        jobs.insert(config.id.clone(), job_id);

        log::info!(
            "[Scheduler] added schedule '{}' ({}): {}",
            config.name,
            config.id,
            config.cron_expression
        );

        Ok(())
    }

    /// Remove a previously-registered schedule by its ID.
    pub async fn remove_schedule(&self, schedule_id: &str) -> Result<(), String> {
        let mut jobs = self.active_jobs.lock().await;
        let job_id = jobs
            .remove(schedule_id)
            .ok_or_else(|| format!("No active schedule with id '{}'", schedule_id))?;

        let sched_guard = self.scheduler.lock().await;
        let scheduler = sched_guard
            .as_ref()
            .ok_or_else(|| "Scheduler not initialized".to_string())?;

        scheduler
            .remove(&job_id)
            .await
            .map_err(|e| format!("Failed to remove job: {}", e))?;

        log::info!("[Scheduler] removed schedule '{}'", schedule_id);
        Ok(())
    }

    /// Return the IDs of all currently active schedules.
    pub async fn list_active(&self) -> Vec<String> {
        let jobs = self.active_jobs.lock().await;
        jobs.keys().cloned().collect()
    }

    /// Gracefully stop the scheduler and clear all tracked jobs.
    pub async fn shutdown(&self) {
        let mut sched_guard = self.scheduler.lock().await;
        if let Some(mut scheduler) = sched_guard.take() {
            if let Err(e) = scheduler.shutdown().await {
                log::error!("[Scheduler] error during shutdown: {}", e);
            } else {
                log::info!("[Scheduler] shut down successfully");
            }
        }
        let mut jobs = self.active_jobs.lock().await;
        jobs.clear();
    }
}
