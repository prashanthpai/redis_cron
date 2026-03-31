//! This is forked from https://github.com/lholden/job_scheduler

extern crate redis_module;
use redis_module::{RedisError, ThreadSafeContext};

use chrono::{offset, DateTime, Duration, Utc};
pub use cron::Schedule;
pub use uuid::Uuid;

pub struct Job {
    job_id: Uuid,
    schedule: Schedule,
    command: String,
    args: Vec<Vec<u8>>,
    limit_missed_runs: usize,
    last_tick: Option<DateTime<Utc>>,
}

pub struct JobInfo {
    pub job_id: String,
    pub schedule: String,
    pub command: String,
    pub args: Vec<Vec<u8>>,
}

pub struct PendingRun {
    job_id: String,
    schedule: String,
    command: String,
    args: Vec<Vec<u8>>,
}

impl Job {
    pub fn new(schedule: Schedule, command: String, args: Vec<Vec<u8>>) -> Job {
        Job {
            job_id: Uuid::new_v4(),
            schedule,
            command,
            args,
            limit_missed_runs: 1,
            last_tick: None,
        }
    }

    fn pending_run(&self) -> PendingRun {
        PendingRun {
            job_id: self.job_id.to_string(),
            schedule: self.schedule.to_string(),
            command: self.command.clone(),
            args: self.args.clone(),
        }
    }

    fn collect_due_runs(&mut self, now: DateTime<Utc>) -> Vec<PendingRun> {
        let mut due_runs = Vec::new();
        let last_tick = match self.last_tick {
            Some(last_tick) => last_tick,
            None => {
                self.last_tick = Some(now);
                return due_runs;
            }
        };

        if self.limit_missed_runs > 0 {
            for event in self.schedule.after(&last_tick).take(self.limit_missed_runs) {
                if event > now {
                    break;
                }
                due_runs.push(self.pending_run());
            }
        } else {
            for event in self.schedule.after(&last_tick) {
                if event > now {
                    break;
                }
                due_runs.push(self.pending_run());
            }
        }

        self.last_tick = Some(now);

        due_runs
    }

    #[allow(dead_code)]
    pub fn limit_missed_runs(&mut self, limit: usize) {
        self.limit_missed_runs = limit;
    }

    #[allow(dead_code)]
    pub fn last_tick(&mut self, last_tick: Option<DateTime<Utc>>) {
        self.last_tick = last_tick;
    }
}

impl PendingRun {
    pub fn run(&self) -> Result<(), RedisError> {
        let args: Vec<&[u8]> = self.args.iter().map(|arg| arg.as_slice()).collect();
        let ctx = ThreadSafeContext::new();
        let tctx = ctx.lock();
        tctx.log_notice(&format!(
            "<cron> run: job_id={}; schedule={}; cmd={};",
            self.job_id, self.schedule, self.command
        ));
        let result = tctx.call(&self.command, args.as_slice()).map(|_| ());
        if let Err(err) = &result {
            tctx.log_warning(&format!(
                "<cron> job failed: job_id={}; schedule={}; cmd={}; err={};",
                self.job_id, self.schedule, self.command, err
            ));
        }
        result
    }
}

#[derive(Default)]
pub struct JobScheduler {
    jobs: Vec<Job>,
}

impl JobScheduler {
    pub fn new() -> JobScheduler {
        JobScheduler { jobs: Vec::new() }
    }

    pub fn add(&mut self, job: Job) -> Uuid {
        let job_id = job.job_id;
        self.jobs.push(job);

        job_id
    }

    pub fn remove(&mut self, job_id: Uuid) -> bool {
        let mut found_index = None;
        for (i, job) in self.jobs.iter().enumerate() {
            if job.job_id == job_id {
                found_index = Some(i);
                break;
            }
        }

        if found_index.is_some() {
            self.jobs.remove(found_index.unwrap());
        }

        found_index.is_some()
    }

    pub fn clear_jobs(&mut self) {
        self.jobs.clear()
    }

    pub fn list_jobs(&self) -> Vec<JobInfo> {
        let mut res = Vec::with_capacity(self.jobs.len());
        for job in &self.jobs {
            res.push(JobInfo {
                job_id: job.job_id.to_string(),
                schedule: job.schedule.to_string(),
                command: job.command.clone(),
                args: job.args.clone(),
            })
        }

        return res;
    }

    pub fn take_due_runs(&mut self) -> Vec<PendingRun> {
        let now = Utc::now();
        let mut due_runs = Vec::new();
        for job in &mut self.jobs {
            due_runs.extend(job.collect_due_runs(now));
        }

        due_runs
    }

    #[allow(dead_code)]
    pub fn time_till_next_job(&self) -> std::time::Duration {
        if self.jobs.is_empty() {
            // Take a guess if there are no jobs.
            return std::time::Duration::from_millis(500);
        }

        let mut duration = Duration::zero();
        let now = Utc::now();
        for job in self.jobs.iter() {
            for event in job.schedule.upcoming(offset::Utc).take(1) {
                let d = event - now;
                if duration.is_zero() || d < duration {
                    duration = d;
                }
            }
        }

        duration.to_std().unwrap()
    }
}
