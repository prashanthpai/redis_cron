//! This is forked from https://github.com/lholden/job_scheduler

extern crate redis_module;

use redis_module::{Context, RedisError, RedisResult, Status, ThreadSafeContext};

use chrono::{offset, DateTime, Duration, Utc};
pub use cron::Schedule;
use std::mem::drop;
pub use uuid::Uuid;

pub struct Job<'a> {
    schedule: Schedule,
    run: Box<dyn (FnMut() -> ()) + Send + Sync + 'a>,
    last_tick: Option<DateTime<Utc>>,
    limit_missed_runs: usize,
    job_id: Uuid,
    args: Vec<String>,
}

impl<'a> Job<'a> {
    pub fn new<T>(schedule: Schedule, run: T, args: Vec<String>) -> Job<'a>
    where
        T: 'a,
        T: FnMut() -> () + Send + Sync,
    {
        Job {
            schedule,
            run: Box::new(run),
            last_tick: None,
            limit_missed_runs: 1,
            job_id: Uuid::new_v4(),
            args: args,
        }
    }

    fn tick(&mut self) {
        let now = Utc::now();
        if self.last_tick.is_none() {
            self.last_tick = Some(now);
            return;
        }
        if self.limit_missed_runs > 0 {
            for event in self
                .schedule
                .after(&self.last_tick.unwrap())
                .take(self.limit_missed_runs)
            {
                if event > now {
                    break;
                }
                let eval_args: Vec<&str> = self.args[3..].iter().map(|s| &s[..]).collect();
                let thread_ctx = ThreadSafeContext::new();
                let tctx = thread_ctx.lock();
                tctx.call(&self.args[2], &eval_args).unwrap();
                drop(tctx);
            }
        } else {
            for event in self.schedule.after(&self.last_tick.unwrap()) {
                if event > now {
                    break;
                }
                let eval_args: Vec<&str> = self.args[3..].iter().map(|s| &s[..]).collect();
                let thread_ctx = ThreadSafeContext::new();
                let tctx = thread_ctx.lock();
                tctx.call(&self.args[2], &eval_args).unwrap();
                drop(tctx);
            }
        }

        self.last_tick = Some(now);
    }

    pub fn limit_missed_runs(&mut self, limit: usize) {
        self.limit_missed_runs = limit;
    }

    pub fn last_tick(&mut self, last_tick: Option<DateTime<Utc>>) {
        self.last_tick = last_tick;
    }
}

#[derive(Default)]
pub struct JobScheduler<'a> {
    jobs: Vec<Job<'a>>,
}

impl<'a> JobScheduler<'a> {
    pub fn new() -> JobScheduler<'a> {
        JobScheduler { jobs: Vec::new() }
    }

    pub fn add(&mut self, job: Job<'a>) -> Uuid {
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

    pub fn tick(&mut self) {
        for job in &mut self.jobs {
            job.tick();
        }
    }

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