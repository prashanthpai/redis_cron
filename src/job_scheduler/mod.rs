//! This is forked from https://github.com/lholden/job_scheduler

extern crate redis_module;
use redis_module::ThreadSafeContext;

use chrono::{offset, DateTime, Duration, Utc};
pub use cron::Schedule;
pub use uuid::Uuid;

pub struct Job {
    job_id: Uuid,
    schedule: Schedule,
    cmd_args: Vec<String>,
    limit_missed_runs: usize,
    last_tick: Option<DateTime<Utc>>,
}

impl Job {
    pub fn new(schedule: Schedule, args: Vec<String>) -> Job {
        Job {
            job_id: Uuid::new_v4(),
            schedule,
            cmd_args: args,
            limit_missed_runs: 1,
            last_tick: None,
        }
    }

    fn run(&self) {
        let args: Vec<&str> = self.cmd_args[1..].iter().map(|s| &s[..]).collect();
        let ctx = ThreadSafeContext::new();
        let tctx = ctx.lock();
        tctx.log_notice(&format!(
            "redis_cron: run: job_id={}; schedule={}; cmd={};",
            self.job_id, self.schedule, self.cmd_args[0]
        ));
        tctx.call(&self.cmd_args[0], &args).unwrap();
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
                self.run();
            }
        } else {
            for event in self.schedule.after(&self.last_tick.unwrap()) {
                if event > now {
                    break;
                }
                self.run();
            }
        }

        self.last_tick = Some(now);
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

    pub fn tick(&mut self) {
        for job in &mut self.jobs {
            job.tick();
        }
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
