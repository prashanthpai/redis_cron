#[macro_use]
extern crate redis_module;

use lazy_static::lazy_static;
use redis_module::{Context, RedisError, RedisResult, RedisString, RedisValue, Status};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::{thread, time};

mod job_scheduler;
use crate::job_scheduler::{Job, JobScheduler, Uuid};

const SCHED_SLEEP_MS: u64 = 500;

lazy_static! {
    static ref SCHED: Mutex<JobScheduler> = Mutex::new(JobScheduler::new());
    static ref TICK_THREAD: Mutex<Option<thread::JoinHandle<()>>> = Mutex::new(None);
    static ref TICK_THREAD_STOP: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
}

fn cron_schedule(ctx: &Context, args: Vec<RedisString>) -> RedisResult {
    if args.len() < 3 {
        return Err(RedisError::WrongArity);
    }
    ctx.auto_memory();

    let schedule = args[1].try_as_str()?.parse()?;
    let command = args[2].try_as_str()?.to_owned();
    let cmd_args = args[3..]
        .iter()
        .map(|arg| Ok(arg.as_slice().to_vec()))
        .collect::<Result<Vec<_>, RedisError>>()?;

    let job_id = SCHED
        .lock()
        .unwrap()
        .add(Job::new(schedule, command, cmd_args))
        .to_string();

    Ok(job_id.into())
}

fn cron_unschedule(ctx: &Context, args: Vec<RedisString>) -> RedisResult {
    if args.len() != 2 {
        return Err(RedisError::WrongArity);
    }
    ctx.auto_memory();

    let job_id = match Uuid::parse_str(args[1].try_as_str()?) {
        Ok(v) => v,
        // return 0 if UUID is invalid
        Err(_err) => return Ok(RedisValue::Integer(false.into())),
    };

    let present = SCHED.lock().unwrap().remove(job_id);

    Ok(RedisValue::Integer(present.into()))
}

fn cron_list(ctx: &Context, args: Vec<RedisString>) -> RedisResult {
    if args.len() != 1 {
        return Err(RedisError::WrongArity);
    }
    ctx.auto_memory();

    let jobs = SCHED.lock().unwrap().list_jobs();
    let mut response = Vec::with_capacity(jobs.len());
    for job in jobs {
        let mut cmd = Vec::with_capacity(job.args.len() + 1);
        cmd.push(RedisValue::SimpleString(job.command));
        cmd.extend(job.args.into_iter().map(RedisValue::StringBuffer));
        response.push(RedisValue::Array(vec![
            RedisValue::SimpleString(job.job_id),
            RedisValue::SimpleString(job.schedule),
            RedisValue::Array(cmd),
        ]))
    }

    Ok(RedisValue::Array(response))
}

fn init(ctx: &Context, _: &[RedisString]) -> Status {
    // TODO: load schedules and commands from stored RDB file
    // if available.
    if TICK_THREAD_STOP.load(Ordering::SeqCst) {
        // if the thread is already stopped, return success
        return Status::Ok;
    }

    *TICK_THREAD.lock().unwrap() = Some(thread::spawn(move || loop {
        if TICK_THREAD_STOP.load(Ordering::SeqCst) {
            return;
        }

        let due_runs = SCHED.lock().unwrap().take_due_runs();
        for due_run in due_runs {
            let _ = due_run.run();
        }

        if TICK_THREAD_STOP.load(Ordering::SeqCst) {
            return;
        }
        thread::sleep(time::Duration::from_millis(SCHED_SLEEP_MS));
    }));
    ctx.log_notice("spawned tick thread");

    Status::Ok
}

fn deinit(ctx: &Context) -> Status {
    TICK_THREAD_STOP.store(true, Ordering::SeqCst);
    ctx.log_notice("signalled tick thread to stop");

    ctx.log_notice("waiting for tick thread to stop");
    if let Some(handle) = TICK_THREAD.lock().unwrap().take() {
        match handle.join() {
            Ok(_) => ctx.log_notice("tick thread stopped gracefully"),
            Err(_) => ctx.log_warning("tick thread panicked"),
        }
    }
    TICK_THREAD_STOP.store(false, Ordering::SeqCst);

    // clear all jobs; this can be made optional on future
    SCHED.lock().unwrap().clear_jobs();

    Status::Ok
}

redis_module! {
    name: "cron",
    version: 1,
    allocator: (redis_module::alloc::RedisAlloc, redis_module::alloc::RedisAlloc),
    data_types: [],
    init: init,
    deinit: deinit,
    commands: [
        ["cron.schedule", cron_schedule, "write deny-oom", 0, 0, 0, ""],
        ["cron.unschedule", cron_unschedule, "write deny-oom", 0, 0, 0, ""],
        ["cron.list", cron_list, "readonly", 0, 0, 0, ""],
    ],
}
