#[macro_use]
extern crate redis_module;

use lazy_static::lazy_static;
use redis_module::{Context, RedisError, RedisResult, RedisValue, Status};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::{thread, time};

mod job_scheduler;
use crate::job_scheduler::{Job, JobScheduler, Uuid};

static mut TICK_THREAD: Option<thread::JoinHandle<()>> = None;
const SCHED_SLEEP_MS: u64 = 500;

lazy_static! {
    static ref SCHED: Mutex<JobScheduler> = Mutex::new(JobScheduler::new());
    static ref TICK_THREAD_STOP: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

fn cron_schedule(ctx: &Context, args: Vec<String>) -> RedisResult {
    if args.len() < 3 {
        return Err(RedisError::WrongArity);
    }
    ctx.auto_memory();

    let job_id = SCHED
        .lock()
        .unwrap()
        .add(Job::new(args[1].parse()?, args[2..].to_vec()))
        .to_string();

    return Ok(job_id.into());
}

fn cron_unschedule(ctx: &Context, args: Vec<String>) -> RedisResult {
    if args.len() != 2 {
        return Err(RedisError::WrongArity);
    }
    ctx.auto_memory();

    let job_id = match Uuid::parse_str(&args[1]) {
        Ok(v) => v,
        // return 0 if UUID is invalid
        Err(_err) => return Ok(RedisValue::Integer(false.into())),
    };

    let present = SCHED.lock().unwrap().remove(job_id);

    return Ok(RedisValue::Integer(present.into()));
}

fn cron_list(ctx: &Context, args: Vec<String>) -> RedisResult {
    if args.len() != 1 {
        return Err(RedisError::WrongArity);
    }
    ctx.auto_memory();

    let jobs = SCHED.lock().unwrap().list_jobs();
    let mut response = Vec::with_capacity(jobs.len());
    for job in jobs {
        response.push(RedisValue::Array(vec![
            RedisValue::SimpleString(job.job_id.into()),
            RedisValue::SimpleString(job.schedule.into()),
            RedisValue::SimpleString(job.cmd_args.into()),
        ]))
    }

    return Ok(RedisValue::Array(response.into()));
}

fn init(ctx: &Context, _: &Vec<String>) -> Status {
    // TODO: load schedules and commands from stored RDB file
    // if available.
    if TICK_THREAD_STOP.load(Ordering::SeqCst) {
        // if the thread is already stopped, return success
        return Status::Ok;
    }

    unsafe {
        TICK_THREAD = Some(thread::spawn(move || loop {
            SCHED.lock().unwrap().tick();
            if TICK_THREAD_STOP.load(Ordering::SeqCst) {
                return;
            }
            thread::sleep(time::Duration::from_millis(SCHED_SLEEP_MS));
        }));
    }
    ctx.log_notice("spawned tick thread");

    Status::Ok
}

fn deinit(ctx: &Context) -> Status {
    TICK_THREAD_STOP.store(true, Ordering::SeqCst);
    ctx.log_notice("signalled tick thread to stop");

    ctx.log_notice("waiting for tick thread to stop");
    unsafe {
        match TICK_THREAD.take().unwrap().join() {
            Ok(_) => ctx.log_notice("tick thread stopped gracefully"),
            Err(_) => ctx.log_warning("tick thread panicked"),
        }
        TICK_THREAD = None;
    }
    TICK_THREAD_STOP.store(false, Ordering::SeqCst);

    // clear all jobs; this can be made optional on future
    SCHED.lock().unwrap().clear_jobs();

    Status::Ok
}

redis_module! {
    name: "cron",
    version: 1,
    data_types: [],
    init: init,
    deinit: deinit,
    commands: [
        ["cron.schedule", cron_schedule, "write deny-oom", 0, 0, 0],
        ["cron.unschedule", cron_unschedule, "write deny-oom", 0, 0, 0],
        ["cron.list", cron_list, "readonly", 0, 0, 0],
    ],
}
